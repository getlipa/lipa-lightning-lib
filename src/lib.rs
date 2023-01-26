#![allow(clippy::let_unit_value)]

mod callbacks;
mod eel_interface_impl;
mod native_logger;

use crate::callbacks::{CallbackError, EventsCallback, LspCallback};
use crate::eel_interface_impl::{EventsImpl, LspImpl, RemoteStorageMock};
use eel::config::Config;
use eel::errors::{LipaError, LipaResult, RuntimeErrorCode};
use eel::keys_manager::{generate_secret, mnemonic_to_secret};
use eel::lsp::LspFee;
use eel::node_info::{ChannelsInfo, NodeInfo};
use eel::secret::Secret;
use eel::InvoiceDetails;
use eel::LogLevel;
use eel::Network;
use native_logger::init_native_logger_once;
use std::sync::Arc;
use storage_mock::Storage;

pub struct LightningNode {
    core_node: eel::LightningNode,
}

impl LightningNode {
    pub fn new(
        config: &Config,
        lsp_callback: Box<dyn LspCallback>,
        events_callback: Box<dyn EventsCallback>,
    ) -> LipaResult<Self> {
        let remote_storage = Box::new(RemoteStorageMock::new(Arc::new(Storage::new())));
        let lsp_client = Box::new(LspImpl { lsp_callback });
        let user_event_handler = Box::new(EventsImpl { events_callback });
        let core_node =
            eel::LightningNode::new(config, remote_storage, lsp_client, user_event_handler)?;
        Ok(LightningNode { core_node })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        self.core_node.get_node_info()
    }

    pub fn query_lsp_fee(&self) -> LipaResult<LspFee> {
        self.core_node.query_lsp_fee()
    }

    pub fn create_invoice(&self, amount_msat: u64, description: String) -> LipaResult<String> {
        self.core_node.create_invoice(amount_msat, description)
    }

    pub fn decode_invoice(&self, invoice: String) -> LipaResult<InvoiceDetails> {
        self.core_node.decode_invoice(invoice)
    }

    pub fn pay_invoice(&self, invoice: String) -> LipaResult<()> {
        self.core_node.pay_invoice(invoice)
    }

    pub fn foreground(&self) {
        self.core_node.foreground()
    }

    pub fn background(&self) {
        self.core_node.background()
    }
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
