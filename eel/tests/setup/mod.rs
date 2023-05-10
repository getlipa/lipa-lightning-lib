#[path = "../mocked_remote_storage/mod.rs"]
pub mod mocked_remote_storage;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::setup_env::config::get_testing_config;
use crate::setup_env::nigiri;
use crate::setup_env::nigiri::NodeInstance;
use crate::setup_env::nigiri::NodeInstance::LspdLnd;
use crate::wait_for_eq;
use eel::config::Config;
use eel::interfaces::{ExchangeRateProvider, RemoteStorage};
use eel::recovery::recover_lightning_node;
use eel::LightningNode;
use mocked_remote_storage::MockedRemoteStorage;
use print_events_handler::PrintEventsHandler;
use std::fs;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Instant;
use storage_mock::Storage;

const LSPD_LND_HOST: &str = "lspd-lnd";
const LSPD_LND_PORT: u16 = 9739;
const REBALANCE_AMOUNT: u64 = 50_000_000; // Msats to be sent to the Lipa node to generate outbound capacity
const CHANNEL_SIZE: u64 = 1_000_000_000; // The capacity of the channel opened by the LSP: See https://github.com/getlipa/lipa-lightning-lib/blob/5657ff45fdf0c45065025d4ff9cb4ab97a32e9f3/lspd/compose.yaml#L54

#[allow(dead_code)]
pub struct NodeHandle<S: RemoteStorage + Clone + 'static> {
    config: Config,
    storage: S,
}

#[allow(dead_code)]
impl<S: RemoteStorage + Clone + 'static> NodeHandle<S> {
    pub fn new(remote_storage: S) -> Self {
        std::env::set_var("TESTING_TASK_PERIODS", "5");
        let config = get_testing_config();
        let _ = fs::remove_dir_all(&config.local_persistence_path);
        fs::create_dir(&config.local_persistence_path).unwrap();

        NodeHandle {
            config,
            storage: remote_storage,
        }
    }

    pub fn start(&self) -> eel::errors::Result<LightningNode> {
        log::debug!("Starting eel node ...");
        let events_handler = PrintEventsHandler {};

        LightningNode::new(
            self.config.clone(),
            Box::new(self.storage.clone()),
            Box::new(events_handler),
            Box::new(ExchangeRateProviderMock {}),
        )
    }

    pub fn start_or_panic(&self) -> LightningNode {
        let start = Instant::now();
        let node = self.start();

        let end = Instant::now();
        log::debug!(
            "Eel node started. Elapsed time: {:?}",
            end.duration_since(start)
        );

        node.unwrap()
    }

    pub fn get_storage(&mut self) -> &mut S {
        &mut self.storage
    }

    pub fn recover(&self) -> eel::errors::Result<()> {
        recover_lightning_node(
            self.config.seed,
            self.config.local_persistence_path.clone(),
            Box::new(self.storage.clone()),
        )
    }
}

#[allow(dead_code)]
pub fn setup_outbound_capacity(node: &LightningNode) {
    wait_for_eq!(node.get_node_info().num_peers, 1);
    nigiri::initiate_channel_from_remote(node.get_node_info().node_pubkey, LspdLnd);

    assert!(node.get_node_info().channels_info.num_channels > 0);
    assert!(node.get_node_info().channels_info.num_usable_channels > 0);
    assert!(node.get_node_info().channels_info.inbound_capacity_msat > REBALANCE_AMOUNT);

    let invoice_details = node
        .create_invoice(REBALANCE_AMOUNT, "test".to_string(), String::new())
        .unwrap();
    assert!(invoice_details.invoice.starts_with("lnbc"));

    nigiri::pay_invoice(LspdLnd, &invoice_details.invoice).unwrap();

    assert_eq!(
        node.get_node_info().channels_info.local_balance_msat,
        REBALANCE_AMOUNT
    );
    assert!(node.get_node_info().channels_info.outbound_capacity_msat < REBALANCE_AMOUNT); // because of channel reserves
    assert!(
        node.get_node_info().channels_info.inbound_capacity_msat < CHANNEL_SIZE - REBALANCE_AMOUNT
    ); // smaller instead of equal because of channel reserves
}

#[allow(dead_code)]
pub fn issue_invoice(node: &LightningNode, payment_amount: u64) -> String {
    let invoice_details = node
        .create_invoice(payment_amount, "test".to_string(), String::new())
        .unwrap();
    assert!(invoice_details.invoice.starts_with("lnbc"));

    invoice_details.invoice
}

#[allow(dead_code)]
pub fn connect_node_to_lsp(node: NodeInstance, lsp_node_id: &str) {
    nigiri::node_connect(node, lsp_node_id, LSPD_LND_HOST, LSPD_LND_PORT).unwrap();
}

#[allow(dead_code)]
pub fn mocked_storage_node() -> NodeHandle<MockedRemoteStorage> {
    mocked_storage_node_configurable(mocked_remote_storage::Config::default())
}

#[allow(dead_code)]
pub fn mocked_storage_node_configurable(
    config: mocked_remote_storage::Config,
) -> NodeHandle<MockedRemoteStorage> {
    let storage = MockedRemoteStorage::new(Arc::new(Storage::new()), config);
    NodeHandle::new(storage)
}

struct ExchangeRateProviderMock;
impl ExchangeRateProvider for ExchangeRateProviderMock {
    fn query_exchange_rate(&self, _code: String) -> eel::errors::Result<u32> {
        Ok(1234)
    }
}
