use bitcoin::Network;
use log::debug;
use simplelog::SimpleLogger;
use std::env;
use uniffi_lipalightninglib::callbacks::RedundantStorageCallback;
use uniffi_lipalightninglib::config::{Config, NodeAddress};
use uniffi_lipalightninglib::keys_manager::generate_secret;
use uniffi_lipalightninglib::LightningNode;

use storage_mock::Storage;

#[derive(Debug)]
pub struct StorageMock {
    storage: Storage,
}

impl StorageMock {
    pub fn new() -> Self {
        Self {
            storage: Storage::new(),
        }
    }
}

impl RedundantStorageCallback for StorageMock {
    fn object_exists(&self, bucket: String, key: String) -> bool {
        self.storage.object_exists(bucket, key)
    }

    fn get_object(&self, bucket: String, key: String) -> Vec<u8> {
        self.storage.get_object(bucket, key)
    }

    fn check_health(&self, bucket: String) -> bool {
        self.storage.check_health(bucket)
    }

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> bool {
        self.storage.put_object(bucket, key, value)
    }

    fn list_objects(&self, bucket: String) -> Vec<String> {
        self.storage.list_objects(bucket)
    }
}

pub fn setup() -> LightningNode {
    SimpleLogger::init(simplelog::LevelFilter::Debug, simplelog::Config::default()).unwrap();

    if env::var("LSP_NODE_PUB_KEY").is_err() {
        // Assume not running in CI. Load .env from example node instead.
        dotenv::from_path("examples/node/.env").unwrap();
    }

    debug!(
        "LSP_NODE_PUB_KEY: {}",
        env::var("LSP_NODE_PUB_KEY").unwrap()
    );
    debug!(
        "LSP_NODE_ADDRESS: {}",
        env::var("LSP_NODE_ADDRESS").unwrap()
    );

    let storage = Box::new(StorageMock::new());

    let config = Config {
        network: Network::Regtest,
        seed: generate_secret("".to_string()).unwrap().seed,
        esplora_api_url: "http://localhost:30000".to_string(),
        lsp_node: NodeAddress {
            pub_key: env::var("LSP_NODE_PUB_KEY").unwrap(),
            address: env::var("LSP_NODE_ADDRESS").unwrap(),
        },
    };

    LightningNode::new(&config, storage).unwrap()
}
