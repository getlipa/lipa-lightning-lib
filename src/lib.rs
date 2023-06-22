#![allow(clippy::let_unit_value)]

extern crate core;

mod amount;
mod callbacks;
mod config;
mod eel_interface_impl;
mod environment;
mod exchange_rate_provider;
mod invoice_details;
mod native_logger;
mod recovery;
mod sanitize_input;

use crate::amount::ToAmount;
pub use crate::amount::{Amount, FiatValue};
pub use crate::callbacks::EventsCallback;
pub use crate::config::Config;
use crate::eel_interface_impl::{EventsImpl, RemoteStorageGraphql};
use crate::environment::Environment;
pub use crate::environment::EnvironmentCode;
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
pub use crate::invoice_details::InvoiceDetails;
pub use crate::recovery::recover_lightning_node;

pub use eel::config::TzConfig;
use eel::errors::{Error as LnError, PayError, PayErrorCode, PayResult, Result, RuntimeErrorCode};
pub use eel::interfaces::ExchangeRate;
pub use eel::invoice::DecodeInvoiceError;
use eel::key_derivation::derive_key_pair_hex;
pub use eel::payment::FiatValues;
use eel::payment::{PaymentState, PaymentType, TzTime};
use eel::secret::Secret;
use eel::secret::{generate_secret, mnemonic_to_secret, words_by_prefix, MnemonicError};
use eel::LogLevel;
pub use eel::Network;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel};
use native_logger::init_native_logger_once;
use perro::{MapToError, ResultTrait};
use std::sync::Arc;
use std::{env, fs};

const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";

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

pub struct LightningNode {
    exchange_rate_provider: ExchangeRateProviderImpl,
    core_node: eel::LightningNode,
}

impl LightningNode {
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        enable_backtrace();
        fs::create_dir_all(&config.local_persistence_path).map_to_permanent_failure(format!(
            "Failed to create directory: {}",
            config.local_persistence_path,
        ))?;

        let seed = sanitize_input::strong_type_seed(&config.seed)?;

        let environment = Environment::load(config.environment);

        let eel_config = eel::config::Config {
            network: environment.network,
            seed,
            fiat_currency: config.fiat_currency,
            esplora_api_url: environment.esplora_url,
            rgs_url: environment.rgs_url,
            lsp_url: environment.lsp_url,
            lsp_token: environment.lsp_token,
            local_persistence_path: config.local_persistence_path,
            timezone_config: config.timezone_config,
        };

        let auth = Arc::new(build_auth(&seed, environment.backend_url.clone())?);

        let remote_storage = Box::new(RemoteStorageGraphql::new(
            environment.backend_url.clone(),
            environment.backend_health_url.clone(),
            Arc::clone(&auth),
        )?);

        let user_event_handler = Box::new(EventsImpl { events_callback });

        let exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            environment.backend_url.clone(),
            Arc::clone(&auth),
        ));

        let core_node = eel::LightningNode::new(
            eel_config,
            remote_storage,
            user_event_handler,
            exchange_rate_provider,
        )?;

        let exchange_rate_provider = ExchangeRateProviderImpl::new(environment.backend_url, auth);

        Ok(LightningNode {
            exchange_rate_provider,
            core_node,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        let rate = self.get_exchange_rate();
        let node = self.core_node.get_node_info();
        let channels = node.channels_info;
        let channels_info = ChannelsInfo {
            num_channels: channels.num_channels,
            num_usable_channels: channels.num_usable_channels,
            local_balance: channels.local_balance_msat.to_amount_down(&rate),
            total_channel_capacities: channels.total_channel_capacities_msat.to_amount_down(&rate),
            inbound_capacity: channels.inbound_capacity_msat.to_amount_down(&rate),
            outbound_capacity: channels.outbound_capacity_msat.to_amount_down(&rate),
        };
        NodeInfo {
            node_pubkey: node.node_pubkey.serialize().to_vec(),
            num_peers: node.num_peers,
            channels_info,
        }
    }

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        let fee = self.core_node.query_lsp_fee()?;
        let channel_minimum_fee = fee
            .channel_minimum_fee_msat
            .to_amount_up(&self.get_exchange_rate());
        Ok(LspFee {
            channel_minimum_fee,
            channel_fee_permyriad: fee.channel_fee_permyriad,
        })
    }

    pub fn calculate_lsp_fee(&self, amount_sat: u64) -> Result<Amount> {
        let rate = self.get_exchange_rate();
        self.core_node
            .calculate_lsp_fee(amount_sat * 1_000)
            .map(|fee| fee.to_amount_up(&rate))
    }

    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        let rate = self.get_exchange_rate();
        self.core_node
            .get_payment_amount_limits()
            .map(|limits| to_limits(limits, &rate))
    }

    pub fn create_invoice(
        &self,
        amount_sat: u64,
        description: String,
        metadata: String,
    ) -> Result<InvoiceDetails> {
        let rate = self.get_exchange_rate();
        let invoice = self
            .core_node
            .create_invoice(amount_sat * 1000, description, metadata)?;
        Ok(InvoiceDetails::from_local_invoice(invoice, &rate))
    }

    pub fn decode_invoice(
        &self,
        invoice: String,
    ) -> std::result::Result<InvoiceDetails, DecodeInvoiceError> {
        let invoice = self.core_node.decode_invoice(invoice)?;
        let rate = self.get_exchange_rate();
        Ok(InvoiceDetails::from_remote_invoice(invoice, &rate))
    }

    pub fn pay_invoice(&self, invoice: String, metadata: String) -> PayResult<()> {
        self.core_node.pay_invoice(invoice, metadata)
    }

    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_sat: u64,
        metadata: String,
    ) -> PayResult<()> {
        self.core_node
            .pay_open_invoice(invoice, amount_sat * 1000, metadata)
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        self.core_node
            .get_latest_payments(number_of_payments)
            .map(|ps| ps.into_iter().map(to_payment).collect())
    }

    pub fn get_payment(&self, hash: String) -> Result<Payment> {
        self.core_node.get_payment(&hash).map(to_payment)
    }

    pub fn foreground(&self) {
        self.core_node.foreground()
    }

    pub fn background(&self) {
        self.core_node.background()
    }

    pub fn list_currency_codes(&self) -> Result<Vec<String>> {
        self.exchange_rate_provider.list_currency_codes()
    }

    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        self.core_node.get_exchange_rate()
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) {
        self.core_node.change_fiat_currency(fiat_currency);
    }

    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        self.core_node.change_timezone_config(timezone_config);
    }

    pub fn panic_directly(&self) {
        self.core_node.panic_directly();
    }

    pub fn panic_in_background_thread(&self) {
        self.core_node.panic_in_background_thread()
    }

    pub fn panic_in_tokio(&self) {
        self.core_node.panic_in_tokio()
    }
}

pub fn accept_terms_and_conditions(environment: EnvironmentCode, seed: Vec<u8>) -> Result<()> {
    enable_backtrace();
    let environment = Environment::load(environment);
    let seed = sanitize_input::strong_type_seed(&seed)?;
    let auth = build_auth(&seed, environment.backend_url)?;
    auth.accept_terms_and_conditions()
        .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnvailable)
}

fn build_auth(seed: &[u8; 64], graphql_url: String) -> Result<Auth> {
    let auth_keys = derive_key_pair_hex(seed, BACKEND_AUTH_DERIVATION_PATH).lift_invalid_input()?;
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
    .map_to_runtime_error(
        RuntimeErrorCode::GenericError,
        "Failed to build auth client",
    )
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
