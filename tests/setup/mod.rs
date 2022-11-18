#[path = "../lsp_client/mod.rs"]
mod lsp_client;

use lsp_client::LspClient;
use storage_mock::Storage;
use uniffi_lipalightninglib::callbacks::RedundantStorageCallback;
use uniffi_lipalightninglib::config::{Config, NodeAddress};
use uniffi_lipalightninglib::errors::InitializationError;
use uniffi_lipalightninglib::keys_manager::generate_secret;
use uniffi_lipalightninglib::LightningNode;

use bitcoin::Network;
use nigiri::NodeInstance;
use simplelog::SimpleLogger;
use std::sync::{Arc, Once};
use std::thread::sleep;
use std::time::Duration;

static INIT_LOGGER_ONCE: Once = Once::new();

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

    fn delete_object(&self, bucket: String, key: String) -> bool {
        self.storage.delete_object(bucket, key)
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
            rgs_url: "http://localhost:8080/snapshot/".to_string(),
        };

        NodeHandle { config, storage }
    }

    pub fn new_with_lsp_setup() -> NodeHandle {
        nigiri::start();

        // to open multiple channels in the same block, multiple UTXOs are required
        for _ in 0..10 {
            nigiri::fund_node(NodeInstance::LspdLnd, 0.5);
            nigiri::fund_node(NodeInstance::NigiriLnd, 0.5);
            nigiri::fund_node(NodeInstance::NigiriCln, 0.5);
        }

        let lsp_info = nigiri::query_node_info(NodeInstance::LspdLnd).unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9739".to_string(),
        };

        Self::new(lsp_node)
    }

    pub fn start(&self) -> Result<LightningNode, InitializationError> {
        let lsp_address = "http://127.0.0.1:6666".to_string();
        let lsp_auth_token =
            "iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l".to_string();
        let lsp_client = LspClient::build(lsp_address, lsp_auth_token);
        let node = LightningNode::new(
            &self.config,
            Box::new(self.storage.clone()),
            Box::new(lsp_client),
        );

        // Wait for the the P2P background task to connect to the LSP
        sleep(Duration::from_millis(1500));

        node
    }
}

#[cfg(feature = "nigiri")]
#[allow(dead_code)]
pub mod nigiri {
    use super::*;

    use crate::try_cmd_repeatedly;
    use log::debug;
    use std::process::{Command, Output};
    use std::thread::sleep;
    use std::time::Duration;

    #[derive(Clone, Copy, Debug)]
    pub enum NodeInstance {
        NigiriCln,
        NigiriLnd,
        LspdLnd,
    }

    const NIGIRI_CLN_CMD_PREFIX: &[&str] = &["nigiri", "cln"];
    const NIGIRI_LND_CMD_PREFIX: &[&str] = &["nigiri", "lnd"];
    const LSPD_LND_CMD_PREFIX: &[&str] = &["docker", "exec", "lspd-lnd", "lncli"];

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
        start_lspd();
        start_rgs();
    }

    pub fn stop() {
        debug!("RGS server stopping ...");
        stop_rgs();
        debug!("LSPD stopping ...");
        stop_lspd(); // Nigiri cannot be stopped if lspd is still connected to it.
        debug!("NIGIRI stopping ...");
        exec(&["nigiri", "stop", "--delete"]);
    }

    pub fn pause() {
        debug!("RGS server stopping ...");
        stop_rgs();
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
        exec_in_dir(&["docker-compose", "down"], "lspd");
    }

    pub fn start_lspd() {
        debug!("LSP starting ...");
        exec_in_dir(&["docker-compose", "up", "-d", "lspd"], "lspd");
        wait_for_sync(NodeInstance::LspdLnd);
    }

    pub fn stop_rgs() {
        exec_in_dir(&["docker-compose", "down"], "rgs");
    }

    fn start_rgs() {
        debug!("RGS server starting ...");
        exec_in_dir(&["docker-compose", "up", "-d", "rgs"], "rgs");
    }

    fn start_nigiri() {
        debug!("NIGIRI starting ...");
        exec(&["nigiri", "start", "--ci", "--ln"]);
        wait_for_sync(NodeInstance::NigiriLnd);
        wait_for_sync(NodeInstance::NigiriCln);
        wait_for_esplora();
    }

    pub fn wait_for_sync(node: NodeInstance) {
        for _ in 0..10 {
            debug!("{:?} is NOT synced yet, waiting...", node);
            sleep(Duration::from_millis(500));

            if let Ok(info) = query_node_info(node) {
                if info.synced {
                    debug!("{:?} is synced", node);
                    return;
                }
            }
        }

        panic!("Failed to start {:?}. Not synced after 5 sec.", node);
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

    pub fn query_node_info(node: NodeInstance) -> Result<RemoteNodeInfo, String> {
        match node {
            NodeInstance::NigiriCln => query_cln_node_info(node),
            _ => query_lnd_node_info(node),
        }
    }

    fn query_lnd_node_info(node: NodeInstance) -> Result<RemoteNodeInfo, String> {
        let sub_cmd = &["getinfo"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let pub_key = json["identity_pubkey"].as_str().unwrap().to_string();
        let synced = json["synced_to_chain"].as_bool().unwrap();
        Ok(RemoteNodeInfo { synced, pub_key })
    }

    fn query_cln_node_info(node: NodeInstance) -> Result<RemoteNodeInfo, String> {
        let sub_cmd = &["getinfo"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let pub_key = json["id"].as_str().unwrap().to_string();

        let bitcoind_synced = json.get("warning_bitcoind_sync").is_none();
        let lightningd_synced = json.get("warning_lightningd_sync").is_none();
        Ok(RemoteNodeInfo {
            synced: bitcoind_synced && lightningd_synced,
            pub_key,
        })
    }

    pub fn mine_blocks(block_amount: u32) -> Result<(), String> {
        let cmd = &["nigiri", "rpc", "-generate", &block_amount.to_string()];

        let output = exec(cmd);
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd, output));
        }
        Ok(())
    }

    pub fn fund_node(node: NodeInstance, amount_btc: f32) {
        let address = match node {
            NodeInstance::NigiriCln => get_cln_node_funding_address(node).unwrap(),
            _ => get_lnd_node_funding_address(node).unwrap(),
        };
        try_cmd_repeatedly!(
            fund_address,
            10,
            Duration::from_millis(500),
            amount_btc,
            &address
        );
    }

    fn fund_address(amount_btc: f32, address: &str) -> Result<(), String> {
        let cmd = &["nigiri", "faucet", &address, &amount_btc.to_string()];

        let output = exec(cmd);
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd, output));
        }
        Ok(())
    }

    pub fn get_lnd_node_funding_address(node: NodeInstance) -> Result<String, String> {
        let sub_cmd = &["newaddress", "p2wkh"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let address = json["address"].as_str().unwrap().to_string();
        Ok(address)
    }

    pub fn get_cln_node_funding_address(node: NodeInstance) -> Result<String, String> {
        let sub_cmd = &["newaddr"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let address = json["bech32"].as_str().unwrap().to_string();
        Ok(address)
    }

    pub fn node_connect(
        node: NodeInstance,
        target_node_id: &str,
        target_node_host: &str,
        target_port: u16,
    ) -> Result<(), String> {
        let address = format!("{}@{}:{}", target_node_id, target_node_host, target_port);
        let sub_cmd = vec!["connect", &address];
        let cmd = [get_node_prefix(node), &sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }

        Ok(())
    }

    pub fn lnd_node_open_generic_channel(
        node: NodeInstance,
        target_node_id: &str,
        zero_conf: bool,
        private: bool,
    ) -> Result<String, String> {
        let mut sub_cmd = vec!["openchannel", target_node_id, "1000000"];

        if private {
            sub_cmd.insert(1, "--private");
        }

        if zero_conf {
            sub_cmd.insert(2, "--zero_conf");
        }

        let cmd = [get_node_prefix(node), &sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let funding_txid = json["funding_txid"].as_str().unwrap().to_string();

        Ok(funding_txid)
    }

    pub fn lnd_node_open_channel(
        node: NodeInstance,
        target_node_id: &str,
        zero_conf: bool,
    ) -> Result<String, String> {
        lnd_node_open_generic_channel(node, target_node_id, zero_conf, true)
    }

    pub fn lnd_node_open_pub_channel(
        node: NodeInstance,
        target_node_id: &str,
        zero_conf: bool,
    ) -> Result<String, String> {
        lnd_node_open_generic_channel(node, target_node_id, zero_conf, false)
    }

    pub fn cln_node_open_pub_channel(
        node: NodeInstance,
        target_node_id: &str,
    ) -> Result<String, String> {
        let sub_cmd = vec!["fundchannel", target_node_id, "1000000"];
        let cmd = [get_node_prefix(node), &sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let funding_txid = json["txid"].as_str().unwrap().to_string();

        Ok(funding_txid)
    }

    pub fn pay_invoice(node: NodeInstance, invoice: &str) -> Result<(), String> {
        match node {
            NodeInstance::NigiriCln => cln_pay_invoice(node, invoice),
            _ => lnd_pay_invoice(node, invoice),
        }
    }

    pub fn lnd_pay_invoice(node: NodeInstance, invoice: &str) -> Result<(), String> {
        let sub_cmd = &["payinvoice", "--force", invoice];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }

        Ok(())
    }

    pub fn cln_pay_invoice(node: NodeInstance, invoice: &str) -> Result<(), String> {
        let sub_cmd = &["pay", invoice];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }

        Ok(())
    }

    pub fn lnd_node_disconnect_peer(node: NodeInstance, node_id: String) -> Result<(), String> {
        let sub_cmd = &["disconnect", &node_id];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());

        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }

        Ok(())
    }

    pub fn lnd_node_force_close_channel(
        node: NodeInstance,
        funding_txid: String,
    ) -> Result<(), String> {
        let sub_cmd = &["closechannel", "--force", &funding_txid];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }
        let _json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;

        Ok(())
    }

    pub fn node_stop(node: NodeInstance) -> Result<(), String> {
        match node {
            NodeInstance::LspdLnd => {
                stop_lspd();
                Ok(())
            }
            _ => nigiri_node_stop(node),
        }
    }

    pub fn nigiri_node_stop(node: NodeInstance) -> Result<(), String> {
        let sub_cmd = &["stop"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }

        Ok(())
    }

    pub fn exec(params: &[&str]) -> Output {
        exec_in_dir(params, ".")
    }

    fn exec_in_dir(params: &[&str], dir: &str) -> Output {
        let (command, args) = params.split_first().expect("At least one param is needed");
        Command::new(command)
            .current_dir(dir)
            .args(args)
            .output()
            .expect("Failed to run command")
    }

    fn get_node_prefix(node: NodeInstance) -> &'static [&'static str] {
        match node {
            NodeInstance::NigiriCln => NIGIRI_CLN_CMD_PREFIX,
            NodeInstance::NigiriLnd => NIGIRI_LND_CMD_PREFIX,
            NodeInstance::LspdLnd => LSPD_LND_CMD_PREFIX,
        }
    }

    fn produce_cmd_err_msg(cmd: &[&str], output: Output) -> String {
        format!(
            "Command `{}` failed.\nStderr: {}Stdout: {}",
            cmd.join(" "),
            String::from_utf8(output.stderr).unwrap(),
            String::from_utf8(output.stdout).unwrap(),
        )
    }

    #[macro_export]
    macro_rules! try_cmd_repeatedly {
        ($func:path, $retry_times:expr, $interval:expr, $($arg:expr),*) => {{
            let mut retry_times = $retry_times;

            while let Err(e) = $func($($arg),*) {
                retry_times -= 1;

                if retry_times == 0 {
                    panic!("Failed to execute {} after {} tries: {}", stringify!($func), $retry_times, e);
                }
                sleep($interval);
            }
        }};
    }
}
