use bitcoin::Network;
use simplelog::SimpleLogger;

use std::sync::Once;
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

#[allow(dead_code)] // not used by all tests
pub fn setup(lsp_node: NodeAddress) -> Result<LightningNode, InitializationError> {
    START_LOGGER_ONCE.call_once(|| {
        SimpleLogger::init(simplelog::LevelFilter::Debug, simplelog::Config::default()).unwrap();
    });
    let storage = Box::new(StorageMock::new());

    let config = Config {
        network: Network::Regtest,
        seed: generate_secret("".to_string()).unwrap().seed,
        esplora_api_url: "http://localhost:30000".to_string(),
        lsp_node,
    };

    LightningNode::new(&config, storage)
}

#[cfg(feature = "nigiri")]
#[allow(dead_code)]
pub mod nigiri {
    use super::*;

    use log::debug;
    use std::env;
    use std::process::{Command, Output};
    use std::thread::sleep;
    use std::time::Duration;

    #[derive(Debug)]
    pub struct RemoteNodeInfo {
        pub pub_key: String,
        pub synced: bool,
    }

    pub fn start() {
        START_LOGGER_ONCE.call_once(|| {
            SimpleLogger::init(simplelog::LevelFilter::Debug, simplelog::Config::default())
                .unwrap();
        });

        // TODO: Optimization, do not restart nigiri if
        // `jq -r .ci  ~/.nigiri/nigiri.config.json` is true.
        if env::var("RUNNING_ON_CI").is_err() {
            debug!("NIGIRI stopping ...");
            stop();
        }

        debug!("NIGIRI starting ...");
        exec(vec!["nigiri", "start", "--ci", "--ln"]);
        wait_for_sync();
        wait_for_esplora();
    }

    pub fn stop() {
        exec(vec!["nigiri", "stop", "--delete"]);
    }

    fn wait_for_sync() {
        let mut counter = 0;
        while !query_lnd_info().is_ok() {
            counter += 1;
            if counter > 10 {
                panic!("Failed to start nigiri");
            }
            debug!("NIGIRI is NOT synced");
            sleep(Duration::from_millis(500));
        }
        debug!("NIGIRI is synced");
    }

    fn wait_for_esplora() {
        let esplora_client = esplora_client::Builder::new("http://localhost:30000")
            .timeout(30)
            .build_blocking()
            .unwrap();

        let mut i = 0u8;
        while let Err(e) = esplora_client.get_height() {
            if i == 15 {
                panic!("Failed to start NIGIRI: {}", e);
            }
            i += 1;
            sleep(Duration::from_secs(1));
        }
    }

    pub fn query_lnd_info() -> Result<RemoteNodeInfo, String> {
        let output = exec(vec!["nigiri", "lnd", "getinfo"]);
        if !output.status.success() {
            return Err("Command `lnd getinfo` failed".to_string());
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let pub_key = json["identity_pubkey"].as_str().unwrap().to_string();
        let synced = json["synced_to_chain"].as_bool().unwrap();
        Ok(RemoteNodeInfo { synced, pub_key })
    }

    pub fn mine_blocks(block_amount: u32) -> Result<(), String> {
        let output = exec(vec![
            "nigiri",
            "rpc",
            "-generate",
            &block_amount.to_string(),
        ]);
        if !output.status.success() {
            return Err(format!("Command `rpc -generate {}` failed", block_amount));
        }
        Ok(())
    }

    pub fn fund_lnd_node(amount_btc: f32) -> Result<(), String> {
        let output = exec(vec!["nigiri", "faucet", "lnd", &amount_btc.to_string()]);
        if !output.status.success() {
            return Err(format!("Command `faucet lnd {}` failed", amount_btc));
        }
        Ok(())
    }

    pub fn try_cmd_repeatedly<T>(
        f: fn(T) -> Result<(), String>,
        param: T,
        mut retry_times: u8,
        interval: Duration,
    ) -> Result<(), String>
    where
        T: Copy,
    {
        while let Err(e) = f(param) {
            retry_times -= 1;

            if retry_times == 0 {
                return Err(e);
            }
            sleep(interval);
        }

        Ok(())
    }

    pub fn lnd_open_channel(node_id: &str) -> Result<String, String> {
        let output = exec(vec![
            "nigiri",
            "lnd",
            "openchannel",
            "--private",
            node_id,
            "1000000",
        ]);
        if !output.status.success() {
            return Err(format!(
                "Command `lnd openchannel --private {} 1000000` failed",
                node_id
            ));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let funding_txid = json["funding_txid"].as_str().unwrap().to_string();

        Ok(funding_txid)
    }

    pub fn lnd_disconnect_peer(node_id: String) -> Result<(), String> {
        let output = exec(vec!["nigiri", "lnd", "disconnect", &node_id]);
        if !output.status.success() {
            return Err(format!("Command `lnd disconnect {}` failed", node_id));
        }

        Ok(())
    }

    pub fn lnd_force_close_channel(funding_txid: String) -> Result<(), String> {
        let output = exec(vec![
            "nigiri",
            "lnd",
            "closechannel",
            "--force",
            &funding_txid,
        ]);
        if !output.status.success() {
            return Err(format!(
                "Command `lnd closechannel --force {}` failed",
                funding_txid
            ));
        }
        let _json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;

        Ok(())
    }

    pub fn lnd_stop() -> Result<(), String> {
        let output = exec(vec!["nigiri", "lnd", "stop"]);
        if !output.status.success() {
            return Err(String::from("Command `lnd stop` failed"));
        }

        Ok(())
    }

    pub fn exec(params: Vec<&str>) -> Output {
        let (command, args) = params.split_first().expect("At least one param is needed");
        Command::new(command)
            .args(args)
            .output()
            .expect("Failed to run command")
    }
}
