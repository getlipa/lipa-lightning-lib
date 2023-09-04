#![allow(clippy::let_unit_value)]

extern crate core;

mod amount;
mod callbacks;
mod config;
mod environment;
mod errors;
mod exchange_rate_provider;
mod fiat_topup;
mod invoice_details;
mod logger;
mod random;
mod recovery;
mod sanitize_input;
mod secret;

use crate::amount::ToAmount;
pub use crate::amount::{Amount, FiatValue};
pub use crate::callbacks::EventsCallback;
pub use crate::config::{Config, TzConfig, TzTime};
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
pub use crate::errors::{Error as LnError, Result, RuntimeErrorCode};
pub use crate::exchange_rate_provider::{ExchangeRate, ExchangeRateProviderImpl};
pub use crate::invoice_details::InvoiceDetails;
pub use crate::recovery::recover_lightning_node;
use crate::secret::Secret;
use bip39::{Language, Mnemonic};
use bitcoin::Network;
use cipher::generic_array::typenum::U32;

use crate::errors::to_mnemonic_error;
pub use crate::errors::{DecodeInvoiceError, MnemonicError, PayError, PayErrorCode, PayResult};
pub use crate::fiat_topup::TopupCurrency;
use crate::fiat_topup::{FiatTopupInfo, PocketClient};
use bitcoin::hashes::hex::ToHex;
use bitcoin::secp256k1::{PublicKey, SECP256K1};
use bitcoin::util::bip32::{DerivationPath, ExtendedPrivKey};
use breez_sdk_core::{
    BreezEvent, BreezServices, EventListener, GreenlightNodeConfig, NodeConfig, NodeState,
};
use crow::LanguageCode;
use crow::{CountryCode, TopupStatus};
use crow::{OfferManager, TopupInfo};
use email_address::EmailAddress;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel, CustomTermsAndConditions};
use iban::Iban;
use log::trace;
use logger::init_logger_once;
use num_enum::TryFromPrimitive;
use perro::Error::RuntimeError;
use perro::{invalid_input, MapToError, ResultTrait};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;
use std::{env, fs};
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;

const LOG_LEVEL: log::Level = log::Level::Trace;
const LOGS_DIR: &str = "logs";

const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SimpleError {
    #[error("SimpleError: {msg}")]
    Simple { msg: String },
}

pub struct PaymentAmountLimits {
    pub max_receive: Amount,
    pub liquidity_limit: LiquidityLimit,
}

pub enum LiquidityLimit {
    None,
    MaxFreeReceive { amount: Amount },
    MinReceive { amount: Amount },
}

pub struct LspFee {
    pub channel_minimum_fee: Amount,
    pub channel_fee_permyriad: u64,
}

pub struct NodeInfo {
    pub node_pubkey: String,
    pub peers: Vec<String>,
    pub channels_info: ChannelsInfo,
}

pub struct ChannelsInfo {
    // This field will currently always be 0 until Breez SDK exposes more detailed channel information https://github.com/breez/breez-sdk/issues/421
    pub num_channels: u16,
    // This field will currently always be 0 until Breez SDK exposes more detailed channel information https://github.com/breez/breez-sdk/issues/421
    pub num_usable_channels: u16,
    pub local_balance: Amount,
    pub inbound_capacity: Amount,
    pub outbound_capacity: Amount,
    pub total_channel_capacities: Amount,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PaymentType {
    Receiving,
    Sending,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PaymentState {
    Created,
    Succeeded,
    Failed,
    Retried,
    InvoiceExpired,
}

pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    pub fail_reason: Option<PayErrorCode>,
    pub hash: String,
    pub amount: Amount,
    pub invoice_details: InvoiceDetails,
    pub created_at: TzTime,
    pub latest_state_change_at: TzTime,
    pub description: String,
    pub preimage: Option<String>,
    pub network_fees: Option<Amount>,
    pub lsp_fees: Option<Amount>,
    pub offer: Option<OfferKind>,
    pub metadata: String,
}

pub enum MaxRoutingFeeMode {
    Relative { max_fee_permyriad: u16 },
    Absolute { max_fee_amount: Amount },
}

#[derive(Debug)]
pub enum OfferStatus {
    READY,
    FAILED,
    SETTLED,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum OfferKind {
    Pocket {
        id: String,
        exchange_rate: ExchangeRate,
        topup_value_minor_units: u64,
        exchange_fee_minor_units: u64,
        exchange_fee_rate_permyriad: u16,
    },
}

pub struct OfferInfo {
    pub offer_kind: OfferKind,
    pub amount: Amount,
    pub lnurlw: String,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    pub status: OfferStatus,
}

// TODO remove dead code after breez sdk implementation
#[allow(dead_code)]
pub struct LightningNode {
    sdk: Arc<BreezServices>,
    auth: Arc<Auth>,
    fiat_topup_client: PocketClient,
    offer_manager: OfferManager,
    rt: Runtime,
}

struct LipaEventListener;

impl EventListener for LipaEventListener {
    fn on_event(&self, e: BreezEvent) {
        match e {
            BreezEvent::NewBlock { .. } => {}
            BreezEvent::InvoicePaid { .. } => {}
            BreezEvent::Synced => {}
            BreezEvent::PaymentSucceed { .. } => {}
            BreezEvent::PaymentFailed { .. } => {}
            BreezEvent::BackupStarted => {}
            BreezEvent::BackupSucceeded => {}
            BreezEvent::BackupFailed { .. } => {}
        }
    }
}

impl LightningNode {
    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        enable_backtrace();
        fs::create_dir_all(&config.local_persistence_path).map_to_permanent_failure(format!(
            "Failed to create directory: {}",
            &config.local_persistence_path,
        ))?;
        if config.enable_file_logging {
            init_logger_once(
                LOG_LEVEL,
                &Path::new(&config.local_persistence_path).join(LOGS_DIR),
            )?;
        }

        let rt = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("3l-async-runtime")
            .enable_time()
            .enable_io()
            .build()
            .unwrap();

        let environment = Environment::load(config.environment);

        // TODO implement error handling
        let strong_typed_seed = sanitize_input::strong_type_seed(&config.seed).unwrap();
        let auth = Arc::new(build_auth(
            &strong_typed_seed,
            environment.backend_url.clone(),
        )?);

        let mut breez_config = BreezServices::default_config(
            environment.environment_type,
            env!("BREEZ_SDK_API_KEY").to_string(),
            NodeConfig::Greenlight {
                config: GreenlightNodeConfig {
                    partner_credentials: None,
                    invite_code: None,
                },
            },
        );

        breez_config.working_dir = config.local_persistence_path;

        let sdk = rt
            .block_on(BreezServices::connect(
                breez_config,
                config.seed,
                Box::new(LipaEventListener {}),
            ))
            .unwrap();

        let _exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            environment.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let fiat_topup_client = PocketClient::new(environment.pocket_url, Arc::clone(&sdk))?;
        let offer_manager = OfferManager::new(environment.backend_url, Arc::clone(&auth));

        Ok(LightningNode {
            sdk,
            auth,
            fiat_topup_client,
            offer_manager,
            rt,
        })
    }

    pub fn get_node_info(&self) -> Result<NodeInfo> {
        let node_state: NodeState = self.sdk.node_info().map_err(|e| RuntimeError {
            code: RuntimeErrorCode::NodeUnavailable,
            msg: e.to_string(),
        })?;
        let rate = self.get_exchange_rate();

        Ok(NodeInfo {
            node_pubkey: node_state.id,
            peers: node_state.connected_peers,
            channels_info: ChannelsInfo {
                num_channels: 0,
                num_usable_channels: 0,
                local_balance: node_state.channels_balance_msat.to_amount_down(&rate),
                inbound_capacity: node_state.inbound_liquidity_msats.to_amount_down(&rate),
                outbound_capacity: node_state.max_payable_msat.to_amount_down(&rate),
                total_channel_capacities: 0.to_amount_down(&rate),
            },
        })
    }

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<Amount> {
        todo!()
    }

    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn create_invoice(
        &self,
        amount_sat: u64,
        description: String,
        metadata: String,
    ) -> Result<InvoiceDetails> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn decode_invoice(
        &self,
        invoice: String,
    ) -> std::result::Result<InvoiceDetails, DecodeInvoiceError> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn pay_invoice(&self, invoice: String, metadata: String) -> PayResult<()> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_sat: u64,
        metadata: String,
    ) -> PayResult<()> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn get_payment(&self, hash: String) -> Result<Payment> {
        todo!()
    }

    pub fn foreground(&self) {
        todo!()
    }

    pub fn background(&self) {
        todo!()
    }

    pub fn list_currency_codes(&self) -> Vec<String> {
        todo!()
    }

    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        // TODO implement get exchange rate
        None
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn change_fiat_currency(&self, fiat_currency: String) {
        todo!()
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        todo!()
    }

    pub fn accept_pocket_terms_and_conditions(&self) -> Result<()> {
        self.auth
            .accept_custom_terms_and_conditions(CustomTermsAndConditions::Pocket)
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
    }

    pub fn register_fiat_topup(
        &self,
        email: Option<String>,
        user_iban: String,
        user_currency: TopupCurrency,
    ) -> Result<FiatTopupInfo> {
        trace!("register_fiat_topup() - called with - email: {email:?} - user_iban: {user_iban} - user_currency: {user_currency:?}");
        if let Err(e) = user_iban.parse::<Iban>() {
            return Err(invalid_input(format!("Invalid user_iban: {}", e)));
        }

        if let Some(email) = email {
            if let Err(e) = EmailAddress::from_str(&email) {
                return Err(invalid_input(format!("Invalid email: {}", e)));
            }
            self.offer_manager
                .register_email(email)
                .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
        }

        self.offer_manager
            .register_node(self.get_node_info()?.node_pubkey)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;

        self.fiat_topup_client
            .register_pocket_fiat_topup(&user_iban, user_currency)
    }

    pub fn query_available_offers(&self) -> Result<Vec<OfferInfo>> {
        let topup_infos = self
            .offer_manager
            .query_available_topups()
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;
        let rate = self.get_exchange_rate();
        Ok(topup_infos
            .into_iter()
            .map(|o| to_offer(o, &rate))
            .collect())
    }

    // TODO remove unused_variables after breez sdk implementation
    #[allow(unused_variables)]
    pub fn request_offer_collection(&self, offer: OfferInfo) -> Result<String> {
        todo!()
    }

    pub fn register_notification_token(
        &self,
        notification_token: String,
        language_iso_639_1: String,
        country_iso_3166_1_alpha_2: String,
    ) -> Result<()> {
        let language = LanguageCode::from_str(&language_iso_639_1.to_lowercase())
            .map_to_invalid_input("Invalid language code")?;
        let country = CountryCode::for_alpha2(&country_iso_3166_1_alpha_2.to_uppercase())
            .map_to_invalid_input("Invalid country code")?;

        self.offer_manager
            .register_notification_token(notification_token, language, country)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)
    }

    pub fn get_wallet_pubkey_id(&self) -> Option<String> {
        self.auth.get_wallet_pubkey_id()
    }

    pub fn get_payment_uuid(&self, payment_hash: String) -> Result<String> {
        get_payment_uuid(payment_hash)
    }
}

fn to_offer(topup_info: TopupInfo, current_rate: &Option<ExchangeRate>) -> OfferInfo {
    let exchange_rate = ExchangeRate {
        currency_code: topup_info.exchange_rate.currency_code,
        rate: topup_info.exchange_rate.sats_per_unit,
        updated_at: topup_info.exchange_rate.updated_at,
    };

    let status = match topup_info.status {
        TopupStatus::READY => OfferStatus::READY,
        TopupStatus::FAILED => OfferStatus::FAILED,
        TopupStatus::SETTLED => OfferStatus::SETTLED,
    };

    OfferInfo {
        offer_kind: OfferKind::Pocket {
            id: topup_info.id,
            exchange_rate,
            topup_value_minor_units: topup_info.topup_value_minor_units,
            exchange_fee_minor_units: topup_info.exchange_fee_minor_units,
            exchange_fee_rate_permyriad: topup_info.exchange_fee_rate_permyriad,
        },
        amount: (topup_info.amount_sat * 1000).to_amount_down(current_rate),
        lnurlw: topup_info.lnurlw,
        created_at: topup_info.exchange_rate.updated_at,
        expires_at: topup_info.expires_at,
        status,
    }
}

pub fn accept_terms_and_conditions(environment: EnvironmentCode, seed: Vec<u8>) -> Result<()> {
    enable_backtrace();
    let environment = Environment::load(environment);
    // TODO implement error handling
    let seed = sanitize_input::strong_type_seed(&seed).unwrap();
    let auth = build_auth(&seed, environment.backend_url)?;
    auth.accept_terms_and_conditions()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
}

fn derive_secret_from_mnemonic(mnemonic: Mnemonic, passphrase: String) -> Secret {
    let seed = mnemonic.to_seed(&passphrase);
    let mnemonic_string: Vec<String> = mnemonic.word_iter().map(String::from).collect();

    Secret {
        mnemonic: mnemonic_string,
        passphrase,
        seed: seed.to_vec(),
    }
}

pub fn generate_secret(passphrase: String) -> std::result::Result<Secret, SimpleError> {
    let entropy = random::generate_random_bytes::<U32>().map_err(|e| SimpleError::Simple {
        msg: format!("Failed to generate random bytes: {e}"),
    })?;
    let mnemonic = Mnemonic::from_entropy(&entropy).map_err(|e| SimpleError::Simple {
        msg: format!("Failed to generate mnemonic: {e}"),
    })?;

    Ok(derive_secret_from_mnemonic(mnemonic, passphrase))
}

pub fn mnemonic_to_secret(
    mnemonic_string: Vec<String>,
    passphrase: String,
) -> std::result::Result<Secret, MnemonicError> {
    let mnemonic = Mnemonic::from_str(&mnemonic_string.join(" ")).map_err(to_mnemonic_error)?;
    Ok(derive_secret_from_mnemonic(mnemonic, passphrase))
}

pub fn words_by_prefix(prefix: String) -> Vec<String> {
    Language::English
        .words_by_prefix(&prefix)
        .iter()
        .map(|w| w.to_string())
        .collect()
}

fn build_auth(seed: &[u8; 64], graphql_url: String) -> Result<Auth> {
    let auth_keys = derive_key_pair_hex(seed, BACKEND_AUTH_DERIVATION_PATH)
        .lift_invalid_input()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
    let auth_keys = KeyPair {
        secret_key: auth_keys.secret_key,
        public_key: auth_keys.public_key,
    };
    Auth::new(
        graphql_url,
        AuthLevel::Pseudonymous,
        auth_keys,
        generate_keypair(),
    )
    .map_to_permanent_failure("Failed to build auth client")
}

pub fn derive_key_pair_hex(seed: &[u8; 64], derivation_path: &str) -> Result<KeyPair> {
    let master_xpriv = ExtendedPrivKey::new_master(Network::Bitcoin, seed)
        .map_to_invalid_input("Failed to get xpriv from from seed")?;

    let derivation_path = DerivationPath::from_str(derivation_path)
        .map_to_invalid_input("Invalid derivation path")?;

    let derived_xpriv = master_xpriv
        .derive_priv(SECP256K1, &derivation_path)
        .map_to_permanent_failure("Failed to derive keys")?;

    let secret_key = derived_xpriv.private_key.secret_bytes();
    let public_key = PublicKey::from_secret_key(SECP256K1, &derived_xpriv.private_key).serialize();

    Ok(KeyPair {
        secret_key: secret_key.to_vec().to_hex(),
        public_key: public_key.to_hex(),
    })
}

fn get_payment_uuid(payment_hash: String) -> Result<String> {
    let hash = hex::decode(payment_hash).map_to_invalid_input("Invalid payment hash encoding")?;

    Ok(Uuid::new_v5(&Uuid::NAMESPACE_OID, &hash)
        .hyphenated()
        .to_string())
}

pub(crate) fn enable_backtrace() {
    env::set_var("RUST_BACKTRACE", "1");
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use bip39::Mnemonic;
    use perro::Error;
    use std::str::FromStr;

    const PAYMENT_HASH: &str = "0b78877a596f18d5f6effde3dda1df25a5cf20439ff1ac91478d7e518211040f";
    const PAYMENT_UUID: &str = "c6e597bd-0a98-5b46-8e74-f6098f5d16a3";
    const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";
    // Values used for testing were obtained from https://iancoleman.io/bip39
    const MNEMONIC_STR: &str = "between angry ketchup hill admit attitude echo wisdom still barrel coral obscure home museum trick grow magic eagle school tilt loop actress equal law";
    const SEED_HEX: &str = "781bfd3b2c6a5cfa9ed1551303fa20edf12baa5864521e7782d42a1bb15c2a444f7b81785f537bec6e38a533d0dc88e2a7effad7b975dd7c9bca1f9e7117966d";
    const DERIVED_AUTH_SECRET_KEY_HEX: &str =
        "1b64f7c3f7462e3815eacef53ddf18e5623bf8945d065761b05b022f19e60251";
    const DERIVED_AUTH_PUBLIC_KEY_HEX: &str =
        "02549b15801b155d32ca3931665361b1d2997ee531859b2d48cebbc2ccf21aac96";

    #[test]
    pub fn test_payment_uuid() {
        let payment_uuid = get_payment_uuid(PAYMENT_HASH.to_string());

        assert_eq!(payment_uuid, Ok(PAYMENT_UUID.to_string()));
    }

    #[test]
    pub fn test_payment_uuid_invalid_input() {
        let invalid_hash_encoding = get_payment_uuid("INVALID_HEX_STRING".to_string());

        assert!(matches!(
            invalid_hash_encoding,
            Err(Error::InvalidInput { .. })
        ));

        assert_eq!(
            &invalid_hash_encoding.unwrap_err().to_string()[0..43],
            "InvalidInput: Invalid payment hash encoding"
        );
    }

    fn mnemonic_to_seed(mnemonic: &str) -> [u8; 64] {
        let mnemonic = Mnemonic::from_str(mnemonic).unwrap();
        let mut seed = [0u8; 64];
        seed.copy_from_slice(&mnemonic.to_seed("")[0..64]);

        seed
    }

    #[test]
    fn test_derive_auth_key_pair() {
        let seed = mnemonic_to_seed(MNEMONIC_STR);
        assert_eq!(seed.to_hex(), SEED_HEX.to_string());

        let key_pair = derive_key_pair_hex(&seed, BACKEND_AUTH_DERIVATION_PATH).unwrap();

        assert_eq!(key_pair.secret_key, DERIVED_AUTH_SECRET_KEY_HEX.to_string());
        assert_eq!(key_pair.public_key, DERIVED_AUTH_PUBLIC_KEY_HEX.to_string());
    }
}
