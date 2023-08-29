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
mod recovery;
mod sanitize_input;
mod interfaces;

use crate::amount::ToAmount;
pub use crate::amount::{Amount, FiatValue};
pub use crate::callbacks::EventsCallback;
pub use crate::config::Config;
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
pub use crate::errors::{Error as LnError, Result, RuntimeErrorCode};
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::invoice_details::InvoiceDetails;
pub use crate::recovery::recover_lightning_node;

pub use crate::fiat_topup::TopupCurrency;
use crate::fiat_topup::{FiatTopupInfo, PocketClient};
use crow::LanguageCode;
use crow::{CountryCode, TopupStatus};
use crow::{OfferManager, TopupInfo};
use email_address::EmailAddress;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel, CustomTermsAndConditions};
use iban::Iban;
use log::trace;
use logger::init_logger_once;
use perro::{invalid_input, MapToError, ResultTrait};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;
use std::{env, fs};
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
    pub num_channels: u16,
    pub num_usable_channels: u16,
    pub local_balance: Amount,
    pub inbound_capacity: Amount,
    pub outbound_capacity: Amount,
    pub total_channel_capacities: Amount,
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

pub struct OfferInfo {
    pub offer_kind: OfferKind,
    pub amount: Amount,
    pub lnurlw: String,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    pub status: OfferStatus,
}

pub struct LightningNode {
    core_node: Arc<eel::LightningNode>,
    auth: Arc<Auth>,
    fiat_topup_client: PocketClient,
    offer_manager: OfferManager,
}

impl LightningNode {
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        enable_backtrace();
        fs::create_dir_all(&config.local_persistence_path).map_to_permanent_failure(format!(
            "Failed to create directory: {}",
            config.local_persistence_path,
        ))?;
        if config.enable_file_logging {
            init_logger_once(
                LOG_LEVEL,
                &Path::new(&config.local_persistence_path).join(LOGS_DIR),
            )?;
        }
        let seed = sanitize_input::strong_type_seed(&config.seed)
            .map_runtime_error_using(RuntimeErrorCode::from_eel_runtime_error_code)?;

        let environment = Environment::load(config.environment);

        let auth = Arc::new(build_auth(&seed, environment.backend_url.clone())?);

        let user_event_handler = Box::new(EventsImpl { events_callback });

        let exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            environment.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let fiat_topup_client = PocketClient::new(environment.pocket_url, Arc::clone(&core_node))?;
        let offer_manager = OfferManager::new(environment.backend_url, Arc::clone(&auth));

        Ok(LightningNode {
            core_node,
            auth,
            fiat_topup_client,
            offer_manager,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        todo!()
    }

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        todo!()
    }

    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<Amount> {
        todo!()
    }

    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        todo!()
    }

    pub fn create_invoice(
        &self,
        amount_sat: u64,
        description: String,
        metadata: String,
    ) -> Result<InvoiceDetails> {
        todo!()
    }

    pub fn decode_invoice(
        &self,
        invoice: String,
    ) -> std::result::Result<InvoiceDetails, DecodeInvoiceError> {
        todo!()
    }

    pub fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        todo!()
    }

    pub fn pay_invoice(&self, invoice: String, metadata: String) -> PayResult<()> {
        todo!()
    }

    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_sat: u64,
        metadata: String,
    ) -> PayResult<()> {
        todo!()
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        todo!()
    }

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
        todo!()
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) {
        todo!()
    }

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
            .register_node(self.get_node_info().node_pubkey.to_string())
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
    let seed = sanitize_input::strong_type_seed(&seed)
        .map_runtime_error_using(RuntimeErrorCode::from_eel_runtime_error_code)?;
    let auth = build_auth(&seed, environment.backend_url)?;
    auth.accept_terms_and_conditions()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
}

pub fn generate_secret(passphrase: String) -> std::result::Result<Secret, SimpleError> {
    eel::keys_manager::generate_secret(passphrase).map_err(|msg| SimpleError::Simple { msg })
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

fn to_payment(payment: eel::payment::Payment) -> Payment {
    let rate = payment.exchange_rate;
    let amount = match payment.payment_type {
        PaymentType::Receiving => payment.amount_msat.to_amount_down(&rate),
        PaymentType::Sending => payment.amount_msat.to_amount_up(&rate),
    };
    let invoice_details = match payment.payment_type {
        PaymentType::Receiving => InvoiceDetails::from_local_invoice(payment.invoice, &rate),
        PaymentType::Sending => InvoiceDetails::from_remote_invoice(payment.invoice, &rate),
    };
    Payment {
        payment_type: payment.payment_type,
        payment_state: payment.payment_state,
        fail_reason: payment.fail_reason,
        hash: payment.hash,
        amount,
        invoice_details,
        created_at: payment.created_at,
        latest_state_change_at: payment.latest_state_change_at,
        description: payment.description,
        preimage: payment.preimage,
        network_fees: payment.network_fees_msat.map(|fee| fee.to_amount_up(&rate)),
        lsp_fees: payment.lsp_fees_msat.map(|fee| fee.to_amount_up(&rate)),
        offer: payment.offer_kind,
        metadata: payment.metadata,
    }
}

fn to_limits(
    limits: eel::limits::PaymentAmountLimits,
    rate: &Option<ExchangeRate>,
) -> PaymentAmountLimits {
    let liquidity_limit = match limits.liquidity_limit {
        eel::limits::LiquidityLimit::None => LiquidityLimit::None,
        eel::limits::LiquidityLimit::MaxFreeReceive { amount_msat } => {
            LiquidityLimit::MaxFreeReceive {
                amount: amount_msat.to_amount_down(rate),
            }
        }
        eel::limits::LiquidityLimit::MinReceive { amount_msat } => LiquidityLimit::MinReceive {
            amount: amount_msat.to_amount_up(rate),
        },
    };
    PaymentAmountLimits {
        max_receive: limits.max_receive_msat.to_amount_down(rate),
        liquidity_limit,
    }
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
    use perro::Error;

    const PAYMENT_HASH: &str = "0b78877a596f18d5f6effde3dda1df25a5cf20439ff1ac91478d7e518211040f";
    const PAYMENT_UUID: &str = "c6e597bd-0a98-5b46-8e74-f6098f5d16a3";

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
}
