#[path = "../mocked_remote_storage/mod.rs"]
pub mod mocked_remote_storage;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::setup_env::config::get_testing_config;
use eel::config::Config;
use eel::interfaces::{ExchangeRateProvider, RemoteStorage};
use eel::recovery::recover_lightning_node;
use eel::LightningNode;
use mocked_remote_storage::MockedRemoteStorage;
use print_events_handler::PrintEventsHandler;
use std::fs;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use storage_mock::Storage;

#[allow(dead_code)]
pub struct NodeHandle<S: RemoteStorage + Clone + 'static> {
    config: Config,
    storage: S,
}

#[allow(dead_code)]
impl<S: RemoteStorage + Clone + 'static> NodeHandle<S> {
    pub fn new(remote_storage: S) -> Self {
        let config = get_testing_config();
        let _ = fs::remove_dir_all(&config.local_persistence_path);
        fs::create_dir(&config.local_persistence_path).unwrap();

        NodeHandle {
            config: config,
            storage: remote_storage,
        }
    }

    pub fn start(&self) -> eel::errors::Result<LightningNode> {
        let events_handler = PrintEventsHandler {};
        let node = LightningNode::new(
            self.config.clone(),
            Box::new(self.storage.clone()),
            Box::new(events_handler),
            Box::new(ExchangeRateProviderMock {}),
        );

        // Wait for the the P2P background task to connect to the LSP
        sleep(Duration::from_millis(1500));

        node
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
