use bitcoin::Network;
use simplelog::SimpleLogger;

use std::sync::{Arc, Once};
use std::thread::sleep;
use std::time::Duration;
use uniffi_lipalightninglib::callbacks::{LspCallback, RedundantStorageCallback};
use uniffi_lipalightninglib::config::{Config, NodeAddress};
use uniffi_lipalightninglib::errors::LspError;
use uniffi_lipalightninglib::keys_manager::generate_secret;
use uniffi_lipalightninglib::LightningNode;

use storage_mock::Storage;
use uniffi_lipalightninglib::errors::InitializationError;

static INIT_LOGGER_ONCE: Once = Once::new();

pub struct LspMock {}

impl Default for LspMock {
    fn default() -> Self {
        Self {}
    }
}

impl LspCallback for LspMock {
    fn channel_information(&self) -> Result<Vec<u8>, LspError> {
        Err(LspError::Grpc)
    }

    fn register_payment(&self, _bytes: Vec<u8>) -> Result<(), LspError> {
        Err(LspError::Grpc)
    }
}

#[derive(Debug, Clone)]
pub struct StorageMock {
    storage: Arc<Storage>,
}

impl StorageMock {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

impl Default for StorageMock {
    fn default() -> Self {
        Self::new(Arc::new(Storage::new()))
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

#[allow(dead_code)]
pub struct NodeHandle {
    config: Config,
    storage: StorageMock,
}

#[allow(dead_code)]
impl NodeHandle {
    pub fn new(lsp_node: NodeAddress) -> Self {
        INIT_LOGGER_ONCE.call_once(|| {
            SimpleLogger::init(simplelog::LevelFilter::Trace, simplelog::Config::default())
                .unwrap();
        });
        let storage = StorageMock::new(Arc::new(Storage::new()));

        let config = Config {
            network: Network::Regtest,
            seed: generate_secret("".to_string()).unwrap().seed,
            esplora_api_url: "http://localhost:30000".to_string(),
            lsp_node,
        };

        NodeHandle { config, storage }
    }

    pub fn start(&self) -> Result<LightningNode, InitializationError> {
        let node = LightningNode::new(
            &self.config,
            Box::new(self.storage.clone()),
            Box::new(LspMock::default()),
        )

        // Wait for the the P2P background task to connect to the LSP
        sleep(Duration::from_millis(1500));

        node
    }
}

#[cfg(feature = "nigiri")]
#[allow(dead_code)]
pub mod nigiri {
    use super::*;

    use log::debug;
    use std::process::{Command, Output};
    use std::thread::sleep;
    use std::time::Duration;

    #[derive(Debug)]
    pub struct RemoteNodeInfo {
        pub pub_key: String,
        pub synced: bool,
    }

    pub fn start() {
        INIT_LOGGER_ONCE.call_once(|| {
            SimpleLogger::init(simplelog::LevelFilter::Debug, simplelog::Config::default())
                .unwrap();
        });

        // Reset Nigiri state to start on a blank slate
        stop();

        start_nigiri();
    }

    pub fn stop() {
        debug!("LSPD stopping ...");
        stop_lspd(); // Nigiri cannot be stopped if lspd is still connected to it.
        debug!("NIGIRI stopping ...");
        exec(&["nigiri", "stop", "--delete"]);
    }

    pub fn pause() {
        debug!("LSPD stopping ...");
        stop_lspd(); // Nigiri cannot be stopped if lspd is still connected to it.
        debug!("NIGIRI pausing (stopping without resetting)...");
        exec(&["nigiri", "stop"]);
    }

    pub fn resume() {
        start_nigiri();
    }

    pub fn resume_without_ln() {
        debug!("NIGIRI starting without LN...");
        exec(&["nigiri", "start", "--ci"]);
        wait_for_esplora();
    }

    pub fn stop_lspd() {
        // will fail on CI for now, because lspd is actually not setup yet. However this does not interrupt testing.
        exec(&["docker-compose", "-f", "./lspd/docker-compose.yml", "down"]);
    }

    fn start_nigiri() {
        debug!("NIGIRI starting ...");
        exec(&["nigiri", "start", "--ci", "--ln"]);
        wait_for_sync();
        wait_for_esplora();
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
        let output = exec(&["nigiri", "lnd", "getinfo"]);
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
        let output = exec(&["nigiri", "rpc", "-generate", &block_amount.to_string()]);
        if !output.status.success() {
            return Err(format!("Command `rpc -generate {}` failed", block_amount));
        }
        Ok(())
    }

    pub fn fund_lnd_node(amount_btc: f32) -> Result<(), String> {
        let output = exec(&["nigiri", "faucet", "lnd", &amount_btc.to_string()]);
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
        let output = exec(&[
            "nigiri",
            "lnd",
            "openchannel",
            "--private",
            node_id,
            "1000000",
        ]);
        if !output.status.success() {
            return Err(format!(
                "Command `lnd openchannel --private {} 1000000` failed: {}",
                node_id,
                String::from_utf8(output.stderr).unwrap()
            ));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let funding_txid = json["funding_txid"].as_str().unwrap().to_string();

        Ok(funding_txid)
    }

    pub fn lnd_disconnect_peer(node_id: String) -> Result<(), String> {
        let output = exec(&["nigiri", "lnd", "disconnect", &node_id]);
        if !output.status.success() {
            return Err(format!("Command `lnd disconnect {}` failed", node_id));
        }

        Ok(())
    }

    pub fn lnd_force_close_channel(funding_txid: String) -> Result<(), String> {
        let output = exec(&["nigiri", "lnd", "closechannel", "--force", &funding_txid]);
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
        let output = exec(&["nigiri", "lnd", "stop"]);
        if !output.status.success() {
            return Err(String::from("Command `lnd stop` failed"));
        }

        Ok(())
    }

    pub fn exec(params: &[&str]) -> Output {
        let (command, args) = params.split_first().expect("At least one param is needed");
        Command::new(command)
            .args(args)
            .output()
            .expect("Failed to run command")
    }
}
