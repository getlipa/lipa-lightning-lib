#[path = "../mocked_remote_storage/mod.rs"]
pub mod mocked_remote_storage;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::setup_env::config::get_testing_config;
use crate::setup_env::nigiri::NodeInstance;
use crate::setup_env::nigiri::NodeInstance::LspdLnd;
use crate::setup_env::{nigiri, CHANNEL_SIZE_MSAT};
use crate::wait_for_eq;
use eel::config::Config;
use eel::errors::RuntimeErrorCode;
use eel::interfaces::{ExchangeRate, ExchangeRateProvider, RemoteStorage};
use eel::recovery::recover_lightning_node;
use eel::LightningNode;
use mocked_remote_storage::MockedRemoteStorage;
use perro::runtime_error;
use print_events_handler::PrintEventsHandler;
use std::fs;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Instant, SystemTime};
use storage_mock::Storage;

const LSPD_LND_HOST: &str = "lspd-lnd";
const LSPD_LND_PORT: u16 = 9739;
const REBALANCE_AMOUNT: u64 = 50_000_000; // Msats to be sent to the Lipa node to generate outbound capacity

#[allow(dead_code)]
pub struct NodeHandle<S: RemoteStorage + Clone + 'static, X: ExchangeRateProvider + Clone + 'static>
{
    config: Config,
    storage: S,
    exchange_rate_provider: X,
}

#[allow(dead_code)]
impl<S: RemoteStorage + Clone + 'static, X: ExchangeRateProvider + Clone> NodeHandle<S, X> {
    pub fn new(remote_storage: S, exchange_rate_provider: X) -> Self {
        std::env::set_var("TESTING_TASK_PERIODS", "5");
        let config = get_testing_config();
        let _ = fs::remove_dir_all(&config.local_persistence_path);
        fs::create_dir(&config.local_persistence_path).unwrap();

        NodeHandle {
            config,
            storage: remote_storage,
            exchange_rate_provider,
        }
    }

    pub fn start(&self) -> eel::errors::Result<LightningNode> {
        log::debug!("Starting eel node ...");
        let events_handler = PrintEventsHandler {};

        LightningNode::new(
            self.config.clone(),
            Box::new(self.storage.clone()),
            Box::new(events_handler),
            Box::new(self.exchange_rate_provider.clone()),
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

    pub fn get_exchange_rate_provider(&mut self) -> &mut X {
        &mut self.exchange_rate_provider
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

    let invoice = node
        .create_invoice(REBALANCE_AMOUNT, "test".to_string(), String::new())
        .unwrap();
    assert!(invoice.to_string().starts_with("lnbc"));

    nigiri::pay_invoice(LspdLnd, &invoice.to_string()).unwrap();

    assert_eq!(
        node.get_node_info().channels_info.local_balance_msat,
        REBALANCE_AMOUNT
    );
    assert!(node.get_node_info().channels_info.outbound_capacity_msat < REBALANCE_AMOUNT); // because of channel reserves
    assert!(
        node.get_node_info().channels_info.inbound_capacity_msat
            < CHANNEL_SIZE_MSAT - REBALANCE_AMOUNT
    ); // smaller instead of equal because of channel reserves
}

#[allow(dead_code)]
pub fn issue_invoice(node: &LightningNode, payment_amount: u64) -> String {
    let invoice = node
        .create_invoice(payment_amount, "test".to_string(), String::new())
        .unwrap();
    assert!(invoice.to_string().starts_with("lnbc"));
    invoice.to_string()
}

#[allow(dead_code)]
pub fn connect_node_to_lsp(node: NodeInstance, lsp_node_id: &str) {
    nigiri::node_connect(node, lsp_node_id, LSPD_LND_HOST, LSPD_LND_PORT).unwrap();
}

#[allow(dead_code)]
pub fn mocked_storage_node() -> NodeHandle<MockedRemoteStorage, ExchangeRateProviderMock> {
    mocked_storage_node_configurable(mocked_remote_storage::Config::default())
}

#[allow(dead_code)]
pub fn mocked_storage_node_configurable(
    config: mocked_remote_storage::Config,
) -> NodeHandle<MockedRemoteStorage, ExchangeRateProviderMock> {
    let storage = MockedRemoteStorage::new(Arc::new(Storage::new()), config);
    NodeHandle::new(storage, ExchangeRateProviderMock::default())
}

#[derive(Clone)]
pub struct ExchangeRateProviderMock {
    available: bool,
}

impl Default for ExchangeRateProviderMock {
    fn default() -> Self {
        ExchangeRateProviderMock { available: true }
    }
}

#[allow(dead_code)]
impl ExchangeRateProviderMock {
    pub fn enable(&mut self) {
        self.available = true;
    }

    pub fn disable(&mut self) {
        self.available = false;
    }
}

impl ExchangeRateProvider for ExchangeRateProviderMock {
    fn query_all_exchange_rates(&self) -> eel::errors::Result<Vec<ExchangeRate>> {
        match self.available {
            true => Ok(vec![
                ExchangeRate {
                    currency_code: "USD".to_string(),
                    rate: 1234,
                    updated_at: SystemTime::now(),
                },
                ExchangeRate {
                    currency_code: "EUR".to_string(),
                    rate: 4321,
                    updated_at: SystemTime::now(),
                },
            ]),
            false => Err(runtime_error(
                RuntimeErrorCode::ExchangeRateProviderUnavailable,
                "Mocked exchange rate provider set to unavailable",
            )),
        }
    }
}
