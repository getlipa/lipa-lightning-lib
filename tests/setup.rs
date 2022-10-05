use bitcoin::Network;
use log::debug;
use simplelog::SimpleLogger;
use std::env;
use std::process::{Command, Stdio};
use std::sync::Once;
use std::thread::sleep;
use std::time::Duration;
use uniffi_lipalightninglib::callbacks::RedundantStorageCallback;
use uniffi_lipalightninglib::config::{Config, NodeAddress};
use uniffi_lipalightninglib::keys_manager::generate_secret;
use uniffi_lipalightninglib::LightningNode;

use storage_mock::Storage;
use uniffi_lipalightninglib::errors::InitializationError;

static START_LOGGER_ONCE: Once = Once::new();

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

impl Default for StorageMock {
    fn default() -> Self {
        Self::new()
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

pub fn setup() -> Result<LightningNode, InitializationError> {
    START_LOGGER_ONCE.call_once(|| {
        SimpleLogger::init(simplelog::LevelFilter::Debug, simplelog::Config::default()).unwrap();
    });

    start_nigiri();

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

    LightningNode::new(&config, storage)
}

pub fn start_nigiri() {
    // only start if nigiri is not yet running
    if !is_nigiri_lnd_synced_to_chain() {
        Command::new("nigiri")
            .arg("start")
            .arg("--ln")
            .output()
            .expect("Failed to start Nigiri");

        block_until_nigiri_ready();
    }
}

pub fn shutdown_nigiri() {
    Command::new("nigiri")
        .arg("stop")
        .output()
        .expect("Failed to shutdown Nigiri");
}

fn block_until_nigiri_ready() {
    while !is_nigiri_lnd_synced_to_chain() {
        sleep(Duration::from_millis(100));
    }
}

fn is_nigiri_lnd_synced_to_chain() -> bool {
    let lnd_getinfo_cmd = Command::new("nigiri")
        .arg("lnd")
        .arg("getinfo")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let output = Command::new("jq")
        .arg(".synced_to_chain")
        .stdin(lnd_getinfo_cmd.stdout.unwrap())
        .output()
        .unwrap();

    output.stdout == b"true\n"
}
