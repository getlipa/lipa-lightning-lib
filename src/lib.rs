#![allow(clippy::let_unit_value)]

mod callbacks;
mod config;
mod eel_interface_impl;
mod exchange_rate_provider;
mod native_logger;
mod sanitize_input;

pub use crate::callbacks::{CallbackError, EventsCallback};
pub use crate::config::Config;
use crate::eel_interface_impl::{EventsImpl, RemoteStorageMock};
use crate::exchange_rate_provider::ExchangeRateProviderImpl;

pub use eel::config::TzConfig;
use eel::errors::{Error as LnError, Result, RuntimeErrorCode};
use eel::key_derivation::derive_key_pair_hex;
use eel::keys_manager::{generate_secret, mnemonic_to_secret};
use eel::lsp::LspFee;
use eel::node_info::{ChannelsInfo, NodeInfo};
use eel::payment_store::{FiatValue, Payment, PaymentState, PaymentType, TzTime};
use eel::secret::Secret;
use eel::InvoiceDetails;
use eel::LogLevel;
pub use eel::Network;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel};
use native_logger::init_native_logger_once;
use perro::{MapToError, ResultTrait};
use std::sync::Arc;
use storage_mock::Storage;

const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";

pub struct LightningNode {
    core_node: eel::LightningNode,
}

impl LightningNode {
    pub fn new(config: Config, events_callback: Box<dyn EventsCallback>) -> Result<Self> {
        let seed = sanitize_input::strong_type_seed(&config.seed)?;
        let eel_config = eel::config::Config {
            network: config.network,
            seed,
            esplora_api_url: config.esplora_api_url,
            rgs_url: config.rgs_url,
            lsp_url: config.lsp_url,
            lsp_token: config.lsp_token,
            local_persistence_path: config.local_persistence_path,
            timezone_config: config.timezone_config,
        };
        let remote_storage = Box::new(RemoteStorageMock::new(Arc::new(Storage::new())));
        let user_event_handler = Box::new(EventsImpl { events_callback });

        let auth = Arc::new(build_auth(&seed, config.graphql_url.clone())?);
        let exchange_rate_provider =
            Box::new(ExchangeRateProviderImpl::new(config.graphql_url, auth));

        let core_node = eel::LightningNode::new(
            &eel_config,
            remote_storage,
            user_event_handler,
            exchange_rate_provider,
        )?;
        Ok(LightningNode { core_node })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        self.core_node.get_node_info()
    }

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
        self.core_node.query_lsp_fee()
    }

    pub fn create_invoice(
        &self,
        amount_msat: u64,
        description: String,
        metadata: String,
    ) -> Result<String> {
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
        // TODO: Implement.
        Ok(Vec::new())
    }

    pub fn get_exchange_rate(&self, code: String) -> Result<u32> {
        self.core_node.get_exchange_rate(code)
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
