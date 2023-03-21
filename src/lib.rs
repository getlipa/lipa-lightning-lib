#![allow(clippy::let_unit_value)]

extern crate core;

mod callbacks;
mod config;
mod eel_interface_impl;
mod exchange_rate_provider;
mod native_logger;
mod sanitize_input;

pub use crate::callbacks::EventsCallback;
pub use crate::config::Config;
use crate::eel_interface_impl::{EventsImpl, RemoteStorageGraphql};
use crate::exchange_rate_provider::ExchangeRateProviderImpl;
use std::fs;

pub use eel::config::TzConfig;
use eel::errors::{Error as LnError, Result, RuntimeErrorCode};
pub use eel::interfaces::ExchangeRates;
use eel::key_derivation::derive_key_pair_hex;
use eel::keys_manager::{generate_secret, mnemonic_to_secret, words_by_prefix};
use eel::lsp::LspFee;
use eel::node_info::{ChannelsInfo, NodeInfo};
use eel::payment_store::{FiatValues, Payment, PaymentState, PaymentType, TzTime};
use eel::secret::Secret;
use eel::InvoiceDetails;
use eel::LogLevel;
pub use eel::Network;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel};
use native_logger::init_native_logger_once;
use perro::{MapToError, ResultTrait};
use std::sync::Arc;

const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";

pub struct LightningNode {
    exchange_rate_provider: ExchangeRateProviderImpl,
    core_node: eel::LightningNode,
}

impl LightningNode {
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        fs::create_dir_all(&config.local_persistence_path).map_to_permanent_failure(format!(
            "Failed to create directory: {}",
            config.local_persistence_path,
        ))?;

        let seed = sanitize_input::strong_type_seed(&config.seed)?;

        let eel_config = eel::config::Config {
            network: config.network,
            seed,
            fiat_currency: config.fiat_currency,
            esplora_api_url: config.esplora_api_url,
            rgs_url: config.rgs_url,
            lsp_url: config.lsp_url,
            lsp_token: config.lsp_token,
            local_persistence_path: config.local_persistence_path,
            timezone_config: config.timezone_config,
        };

        let auth = Arc::new(build_auth(&seed, config.graphql_url.clone())?);

        let remote_storage = Box::new(RemoteStorageGraphql::new(
            config.graphql_url.clone(),
            config.backend_health_url,
            Arc::clone(&auth),
        )?);

        let user_event_handler = Box::new(EventsImpl { events_callback });

        let exchange_rate_provider = Box::new(ExchangeRateProviderImpl::new(
            config.graphql_url.clone(),
            Arc::clone(&auth),
        ));

        let core_node = eel::LightningNode::new(
            eel_config,
            remote_storage,
            user_event_handler,
            exchange_rate_provider,
        )?;

        let exchange_rate_provider = ExchangeRateProviderImpl::new(config.graphql_url, auth);

        Ok(LightningNode {
            exchange_rate_provider,
            core_node,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        self.core_node.get_node_info()
    }

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        self.core_node.query_lsp_fee()
    }

    pub fn calculate_lsp_fee(&self, amount_msat: u64) -> Result<u64> {
        self.core_node.calculate_lsp_fee(amount_msat)
    }

    pub fn create_invoice(
        &self,
        amount_msat: u64,
        description: String,
        metadata: String,
    ) -> Result<InvoiceDetails> {
        self.core_node
            .create_invoice(amount_msat, description, metadata)
    }

    pub fn decode_invoice(&self, invoice: String) -> Result<InvoiceDetails> {
        self.core_node.decode_invoice(invoice)
    }

    pub fn pay_invoice(&self, invoice: String, metadata: String) -> Result<()> {
        self.core_node.pay_invoice(invoice, metadata)
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        self.core_node.get_latest_payments(number_of_payments)
    }

    pub fn get_payment(&self, hash: String) -> Result<Payment> {
        self.core_node.get_payment(&hash)
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

    pub fn get_exchange_rates(&self) -> Result<ExchangeRates> {
        self.core_node.get_exchange_rates()
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) {
        self.core_node.change_fiat_currency(fiat_currency);
    }

    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        self.core_node.change_timezone_config(timezone_config);
    }
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

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
