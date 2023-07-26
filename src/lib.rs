#![allow(clippy::let_unit_value)]

extern crate core;

mod amount;
mod backend_client;
mod callbacks;
mod config;
mod eel_interface_impl;
mod environment;
mod errors;
mod exchange_rate_provider;
mod fiat_topup;
mod invoice_details;
mod logger;
mod recovery;
mod sanitize_input;

use crate::amount::ToAmount;
pub use crate::amount::{Amount, FiatValue};
pub use crate::callbacks::EventsCallback;
pub use crate::config::Config;
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
pub use crate::errors::{Error as LnError, Result, RuntimeErrorCode};
pub use crate::invoice_details::InvoiceDetails;
pub use crate::recovery::recover_lightning_node;

use crate::backend_client::BackendClient;
pub use crate::fiat_topup::TopupCurrency;
use crate::fiat_topup::{FiatTopupInfo, PocketClient};
use bitcoin::secp256k1::PublicKey;
pub use eel::config::TzConfig;
use eel::errors::{PayError, PayErrorCode, PayResult};
pub use eel::interfaces::ExchangeRate;
pub use eel::invoice::DecodeInvoiceError;
use eel::key_derivation::derive_key_pair_hex;
use eel::keys_manager::{mnemonic_to_secret, words_by_prefix, MnemonicError};
pub use eel::payment::FiatValues;
use eel::payment::{PaymentState, PaymentType, TzTime};
use eel::secret::Secret;
pub use eel::Network;
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
use std::time::{Instant, SystemTime};
use std::{env, fs};

use breez_sdk_core::{
    parse_invoice, BreezEvent, BreezServices, EnvironmentType, EventListener, GreenlightNodeConfig,
    LNInvoice,
};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

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
    pub node_pubkey: Vec<u8>,
    pub num_peers: u16,
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
    pub metadata: String,
}

pub enum MaxRoutingFeeMode {
    Relative { max_fee_permyriad: u16 },
    Absolute { max_fee_amount: Amount },
}

pub struct OfferInfo {
    pub offer_kind: OfferKind,
    pub amount: Amount,
    pub lnurlw: String,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

pub enum OfferKind {
    Pocket {
        exchange_fee: FiatValue,
        exchange_fee_rate_permyriad: u16,
    },
}

struct LipaEventListener;

impl EventListener for LipaEventListener {
    fn on_event(&self, e: BreezEvent) {
        match e {
            BreezEvent::NewBlock { .. } => {}
            BreezEvent::InvoicePaid { details: _ } => {
                println!("A payment was received!")
            }
            BreezEvent::Synced => {}
            BreezEvent::PaymentSucceed { .. } => {
                println!("A payment as been sent!")
            }
            BreezEvent::PaymentFailed { details } => {
                println!("A payment failed to be send! Error: {}", details.error);
            }
            BreezEvent::BackupStarted => {}
            BreezEvent::BackupSucceeded => {}
            BreezEvent::BackupFailed { .. } => {}
        }
    }
}

pub struct LightningNode {
    auth: Arc<Auth>,
    fiat_topup_client: PocketClient,
    backend_client: BackendClient,
    rt: Runtime,
    sdk: Arc<BreezServices>,
}

impl LightningNode {
    pub fn new(config: Config, _events_callback: Box<dyn EventsCallback>) -> Result<Self> {
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

        let rt = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("breez-async-runtime")
            .enable_time()
            .enable_io()
            .build()
            .unwrap();

        let api_key = env!("BREEZ_SDK_API_KEY").to_string();
        let invite_code = Some(env!("BREEZ_SDK_INVITE_CODE").to_string());
        let mut breez_config = BreezServices::default_config(
            EnvironmentType::Production,
            api_key,
            breez_sdk_core::NodeConfig::Greenlight {
                config: GreenlightNodeConfig {
                    partner_credentials: None,
                    invite_code,
                },
            },
        );
        breez_config.working_dir = config.local_persistence_path;
        breez_config.maxfee_percent = 5.0;

        print!("Calling connect() ... ");
        let now = Instant::now();
        let sdk = rt
            .block_on(BreezServices::connect(
                breez_config,
                config.seed.clone(),
                Box::new(LipaEventListener {}),
            ))
            .unwrap();
        println!("in {}ms", now.elapsed().as_millis());

        // print!("Calling list_lsps() ... ");
        // let now = Instant::now();
        // let lsps = rt.block_on(sdk.list_lsps()).expect("List of LSPs");
        // println!("in {}ms", now.elapsed().as_millis());
        // println!("{:?}", lsps);
        // let lsp = lsps.first().expect("At least one LSP");

        // print!("Calling connect_lsp() ...");
        // let now = Instant::now();
        // rt.block_on(sdk.connect_lsp(lsp.id.clone()))
        //     .expect("Connect to the LSP");
        // println!("in {}ms", now.elapsed().as_millis());

        let fiat_topup_client = PocketClient::new(environment.pocket_url)?;
        let backend_client = BackendClient::new(environment.backend_url, Arc::clone(&auth))?;

        Ok(LightningNode {
            auth,
            rt,
            sdk,
            fiat_topup_client,
            backend_client,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        let breez_info = self.sdk.node_info().unwrap();
        let rate = self.get_exchange_rate();
        let channels_info = ChannelsInfo {
            num_channels: 0,
            num_usable_channels: 0,
            local_balance: breez_info.channels_balance_msat.to_amount_down(&rate),
            inbound_capacity: breez_info.inbound_liquidity_msats.to_amount_down(&rate),
            outbound_capacity: breez_info.max_payable_msat.to_amount_down(&rate),
            total_channel_capacities: (0 as u64).to_amount_down(&rate),
        };
        let node_pubkey = PublicKey::from_str(&breez_info.id)
            .unwrap()
            .serialize()
            .to_vec();
        NodeInfo {
            node_pubkey,
            num_peers: breez_info.connected_peers.len() as u16,
            channels_info,
        }
    }

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        let lsp_info = self.rt.block_on(self.sdk.lsp_info()).unwrap();
        let channel_minimum_fee =
            (lsp_info.channel_minimum_fee_msat as u64).to_amount_up(&self.get_exchange_rate());
        Ok(LspFee {
            channel_minimum_fee,
            channel_fee_permyriad: lsp_info.channel_fee_permyriad as u64,
        })
    }

    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<Amount> {
        let rate = self.get_exchange_rate();
        let lsp_info = self.query_lsp_fee().unwrap();

        if self.get_node_info().channels_info.inbound_capacity.sats >= amount_sat {
            Ok((0 as u64).to_amount_down(&rate))
        } else {
            let fee = amount_sat * lsp_info.channel_fee_permyriad / 10_000;
            if fee > lsp_info.channel_minimum_fee.sats {
                Ok(fee.to_amount_up(&rate))
            } else {
                Ok(lsp_info.channel_minimum_fee)
            }
        }
    }

    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        todo!()
    }

    pub fn create_invoice(
        &self,
        amount_sat: u64,
        description: String,
        _metadata: String,
    ) -> Result<InvoiceDetails> {
        let invoice = self
            .rt
            .block_on(self.sdk.receive_payment(amount_sat, description))
            .map_to_permanent_failure("Failed to create invoice")?;
        let rate = self.get_exchange_rate();
        Ok(to_invoice_details(invoice, &rate))
    }

    pub fn decode_invoice(
        &self,
        invoice: String,
    ) -> std::result::Result<InvoiceDetails, DecodeInvoiceError> {
        let invoice = parse_invoice(&invoice)
            .map_err(|e| DecodeInvoiceError::ParseError { msg: e.to_string() })?;
        let rate = self.get_exchange_rate();
        Ok(to_invoice_details(invoice, &rate))
    }

    pub fn get_payment_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        todo!()
    }

    pub fn pay_invoice(&self, invoice: String, _metadata: String) -> PayResult<()> {
        print!("Calling pay_invoice() ... ");
        let now = Instant::now();
        self.rt
            .block_on(self.sdk.send_payment(invoice, None))
            .map_to_runtime_error(PayErrorCode::UnexpectedError, "Failed to pay invoice")?;
        println!("in {}ms", now.elapsed().as_millis());
        Ok(())
    }

    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_sat: u64,
        _metadata: String,
    ) -> PayResult<()> {
        print!("Calling pay_open_invoice() ... ");
        let now = Instant::now();
        self.rt
            .block_on(self.sdk.send_payment(invoice, Some(amount_sat)))
            .map_to_runtime_error(PayErrorCode::UnexpectedError, "Failed to pay invoice")?;
        println!("in {}ms", now.elapsed().as_millis());

        Ok(())
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        todo!()
    }

    pub fn get_payment(&self, hash: String) -> Result<Payment> {
        todo!()
    }

    pub fn foreground(&self) {}

    pub fn background(&self) {}

    pub fn list_currency_codes(&self) -> Vec<String> {
        todo!()
    }

    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        Some(ExchangeRate {
            currency_code: "EUR".into(),
            rate: 3552,
            updated_at: SystemTime::now(),
        })
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) {}

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
            self.backend_client
                .register_email(email)
                .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)?;
        }

        self.auth
            .register_node(self.sdk.node_info().unwrap().id)
            .map_runtime_error_to(RuntimeErrorCode::TopupServiceUnavailable)?;

        self.fiat_topup_client
            .register_pocket_fiat_topup(&user_iban, user_currency)
    }

    pub fn query_available_offers(&self) -> Result<Vec<OfferInfo>> {
        self.backend_client
            .query_available_topups()
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
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

fn to_invoice_details(invoice: LNInvoice, rate: &Option<ExchangeRate>) -> InvoiceDetails {
    InvoiceDetails {
        invoice: invoice.bolt11,
        amount: invoice.amount_msat.map(|a| a.to_amount_down(rate)),
        description: invoice.description.unwrap(),
        payment_hash: invoice.payment_hash,
        payee_pub_key: invoice.payee_pubkey,
        creation_timestamp: SystemTime::UNIX_EPOCH + Duration::from_secs(invoice.timestamp),
        expiry_interval: Duration::from_secs(invoice.expiry),
        expiry_timestamp: SystemTime::UNIX_EPOCH
            + Duration::from_secs(invoice.timestamp + invoice.expiry),
    }
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

pub(crate) fn enable_backtrace() {
    env::set_var("RUST_BACKTRACE", "1");
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
