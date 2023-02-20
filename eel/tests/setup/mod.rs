#[path = "../config/mod.rs"]
pub mod config;
#[path = "../mocked_remote_storage/mod.rs"]
pub mod mocked_remote_storage;
#[path = "../print_events_handler/mod.rs"]
mod print_event_handler;

use eel::LightningNode;
use std::fs;

use crate::setup::config::{get_testing_config, LOCAL_PERSISTENCE_PATH};
use crate::setup::mocked_remote_storage::MockedRemoteStorage;
#[cfg(feature = "nigiri")]
use crate::setup::nigiri::{NodeInstance, RGS_CLN_HOST, RGS_CLN_ID, RGS_CLN_PORT};
use crate::setup::print_event_handler::PrintEventsHandler;
use eel::config::Config;
use simplelog::{ConfigBuilder, LevelFilter, SimpleLogger};
use std::sync::{Arc, Once};
use std::thread::sleep;
use std::time::Duration;
use storage_mock::Storage;

static INIT_LOGGER_ONCE: Once = Once::new();

#[allow(dead_code)]
pub struct NodeHandle {
    config: Config,
    storage: MockedRemoteStorage,
}

#[ctor::ctor]
fn init_logger() {
    INIT_LOGGER_ONCE.call_once(|| {
        let config = ConfigBuilder::new()
            .add_filter_ignore_str("ureq")
            .add_filter_ignore_str("mio")
            .add_filter_ignore_str("reqwest")
            .add_filter_ignore_str("want")
            .add_filter_ignore_str("h2")
            .add_filter_ignore_str("tracing")
            .add_filter_ignore_str("hyper")
            .add_filter_ignore_str("tonic")
            .add_filter_ignore_str("tokio_util")
            .add_filter_ignore_str("tower")
            .build();
        SimpleLogger::init(LevelFilter::Trace, config).unwrap();
    });
}

#[allow(dead_code)]
impl NodeHandle {
    pub fn new(storage_config: mocked_remote_storage::Config) -> Self {
        let storage = MockedRemoteStorage::new(Arc::new(Storage::new()), storage_config);

        let _ = fs::remove_dir_all(LOCAL_PERSISTENCE_PATH);
        fs::create_dir(LOCAL_PERSISTENCE_PATH).unwrap();

        NodeHandle {
            config: get_testing_config(),
            storage,
        }
    }

    pub fn default() -> Self {
        Self::new(mocked_remote_storage::Config::default())
    }

    pub fn start(&self) -> eel::errors::Result<LightningNode> {
        let events_handler = PrintEventsHandler {};
        let node = LightningNode::new(
            &self.config,
            Box::new(self.storage.clone()),
            Box::new(events_handler),
        );

        // Wait for the the P2P background task to connect to the LSP
        sleep(Duration::from_millis(1500));

        node
    }

    pub fn get_storage(&mut self) -> &mut MockedRemoteStorage {
        &mut self.storage
    }
}

#[cfg(feature = "nigiri")]
fn node_connect_to_rgs_cln(node: NodeInstance) {
    nigiri::node_connect(node, RGS_CLN_ID, RGS_CLN_HOST, RGS_CLN_PORT).unwrap();
}

#[cfg(feature = "nigiri")]
#[allow(dead_code)]
pub mod nigiri {
    use super::*;

    use crate::try_cmd_repeatedly;
    use bitcoin::hashes::hex::ToHex;
    use log::debug;
    use std::process::{Command, Output};
    use std::thread::sleep;
    use std::time::Duration;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum NodeInstance {
        NigiriCln,
        NigiriLnd,
        LspdLnd,
    }

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const NIGIRI_CLN_CMD_PREFIX: &[&str] = &["nigiri", "cln"];
    const NIGIRI_LND_CMD_PREFIX: &[&str] = &["nigiri", "lnd"];
    const LSPD_LND_CMD_PREFIX: &[&str] = &["docker", "exec", "lspd-lnd", "lncli"];

    pub const RGS_CLN_ID: &str =
        "03f3bf54dd54d3cebb21665f8af405261ca8a241938254a46b1ead7b569199f607";
    pub const RGS_CLN_HOST: &str = "rgs-cln";
    pub const RGS_CLN_PORT: u16 = 9937;

    #[derive(Debug)]
    pub struct RemoteNodeInfo {
        pub pub_key: String,
        pub synced: bool,
    }

    pub fn setup_environment_with_lsp() {
        start_all_clean();

        // to open multiple channels in the same block, multiple UTXOs are required
        for _ in 0..10 {
            fund_node(NodeInstance::LspdLnd, 0.5);
            fund_node(NodeInstance::NigiriLnd, 0.5);
            fund_node(NodeInstance::NigiriCln, 0.5);
        }
    }

    pub fn setup_environment_with_lsp_rgs() {
        setup_environment_with_lsp();

        node_connect_to_rgs_cln(NodeInstance::LspdLnd);
        node_connect_to_rgs_cln(NodeInstance::NigiriLnd);
        node_connect_to_rgs_cln(NodeInstance::NigiriCln);
    }

    pub fn start_all_clean() {
        // Reset Nigiri state to start on a blank slate
        stop();

        start_nigiri();
        start_lspd();
        start_rgs();

        wait_for_healthy_nigiri();
        wait_for_healthy_lspd();
    }

    pub fn stop() {
        stop_rgs();
        stop_lspd(); // Nigiri cannot be stopped if lspd is still connected to it.
        debug!("NIGIRI stopping ...");
        exec(&["nigiri", "stop", "--delete"]);
    }

    pub fn pause() {
        stop_rgs();
        stop_lspd(); // Nigiri cannot be stopped if lspd is still connected to it.
        debug!("NIGIRI pausing (stopping without resetting)...");
        exec(&["nigiri", "stop"]);
    }

    pub fn resume() {
        start_nigiri();
        wait_for_healthy_nigiri();
    }

    pub fn resume_without_ln() {
        debug!("NIGIRI starting without LN...");
        exec(&["nigiri", "start", "--ci"]);
        wait_for_esplora();
    }

    pub fn stop_lspd() {
        debug!("LSPD stopping ...");
        exec_in_dir(&["docker-compose", "down"], &lspd_home());
    }

    pub fn pause_lspd() {
        debug!("LSPD stopping ...");
        exec_in_dir(&["docker-compose", "stop"], &lspd_home());
    }

    pub fn start_lspd() {
        debug!("LSPD starting ...");
        exec_in_dir(&["docker-compose", "up", "-d", "lspd"], &lspd_home());
    }

    pub fn wait_for_healthy_lspd() {
        wait_for_sync(NodeInstance::LspdLnd);
    }

    pub fn ensure_environment_running() {
        if is_node_synced(NodeInstance::NigiriLnd) && is_node_synced(NodeInstance::LspdLnd) {
            debug!("Environment is up and running");
        } else {
            setup_environment_with_lsp_rgs();
        }
    }

    pub fn stop_rgs() {
        debug!("RGS server stopping ...");
        exec_in_dir(&["docker-compose", "down"], &rgs_home());
    }

    fn start_rgs() {
        debug!("RGS server starting ...");
        exec_in_dir(&["docker-compose", "up", "-d", "rgs"], &rgs_home());
    }

    fn start_nigiri() {
        debug!("NIGIRI starting ...");
        exec(&["nigiri", "start", "--ci", "--ln"]);
    }

    fn wait_for_healthy_nigiri() {
        wait_for_sync(NodeInstance::NigiriLnd);
        wait_for_sync(NodeInstance::NigiriCln);
        wait_for_esplora();
    }

    pub fn wait_for_sync(node: NodeInstance) {
        for _ in 0..40 {
            if is_node_synced(node) {
                return;
            }
            sleep(Duration::from_millis(500));
        }

        panic!("Failed to start {:?}. Not synced after 20 sec.", node);
    }

    pub fn is_node_synced(node: NodeInstance) -> bool {
        if let Ok(info) = query_node_info(node) {
            if info.synced {
                debug!("{:?} is synced", node);
                return true;
            }
        }

        debug!("{:?} is NOT synced", node);
        false
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

    pub fn query_node_balance(node: NodeInstance) -> Result<u64, String> {
        match node {
            NodeInstance::NigiriCln => query_cln_node_balance(node),
            _ => query_lnd_node_balance(node),
        }
    }

    fn query_lnd_node_balance(node: NodeInstance) -> Result<u64, String> {
        let sub_cmd = &["channelbalance"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let balance = json["local_balance"]["msat"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        Ok(balance)
    }

    fn query_cln_node_balance(node: NodeInstance) -> Result<u64, String> {
        let sub_cmd = &["listfunds"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;

        let channels = json["channels"].as_array().unwrap();
        let mut balance: u64 = 0;
        for channel in channels {
            balance += channel["our_amount_msat"]
                .as_str()
                .unwrap()
                .strip_suffix("msat")
                .unwrap()
                .parse::<u64>()
                .unwrap();
        }

        Ok(balance)
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
        let cmd = &["nigiri", "faucet", address, &amount_btc.to_string()];

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
            let err_msg = String::from_utf8(output.stderr.clone()).unwrap();
            if !err_msg.contains("already connected to peer") {
                return Err(produce_cmd_err_msg(&cmd, output));
            }
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

    pub fn issue_invoice(
        node: NodeInstance,
        description: &str,
        amount_msat: u64,
        expiry: u64,
    ) -> Result<String, String> {
        match node {
            NodeInstance::NigiriCln => cln_issue_invoice(node, description, amount_msat, expiry),
            _ => lnd_issue_invoice(node, description, amount_msat, expiry),
        }
    }

    pub fn lnd_issue_invoice(
        node: NodeInstance,
        description: &str,
        amount_msat: u64,
        expiry: u64,
    ) -> Result<String, String> {
        let sub_cmd = &[
            "addinvoice",
            "--memo",
            description,
            "--amt_msat",
            &amount_msat.to_string(),
            "--expiry",
            &expiry.to_string(),
        ];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }

        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let invoice = json["payment_request"].as_str().unwrap().to_string();

        Ok(invoice)
    }

    pub fn cln_issue_invoice(
        node: NodeInstance,
        description: &str,
        amount_msat: u64,
        expiry: u64,
    ) -> Result<String, String> {
        let sub_cmd = &[
            "invoice",
            &amount_msat.to_string(),
            &rand::random::<u64>().to_string(),
            description,
            &expiry.to_string(),
        ];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(cmd.as_slice(), output));
        }

        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let invoice = json["bolt11"].as_str().unwrap().to_string();

        Ok(invoice)
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

    pub fn list_peers(node: NodeInstance) -> Result<Vec<String>, String> {
        let sub_cmd = &["listpeers"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            return Err(produce_cmd_err_msg(&cmd, output));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;
        let peers = json["peers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|peer| peer["pub_key"].as_str().unwrap().to_string())
            .collect();
        Ok(peers)
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

    pub fn initiate_node_with_channel(remote_node: NodeInstance) -> LightningNode {
        nigiri::setup_environment_with_lsp();

        let node = NodeHandle::default().start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        lnd_node_open_channel(remote_node, &node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);

        wait_for_new_channel_to_confirm(remote_node, &node_id);
        node
    }

    pub fn wait_for_new_channel_to_confirm(node: NodeInstance, target_node_id: &str) {
        let remote_node_json_keyword = match node {
            NodeInstance::NigiriCln => "destination",
            _ => "remote_pubkey",
        };

        let node_id = if node == NodeInstance::NigiriCln {
            Some(nigiri::query_cln_node_info(node).unwrap().pub_key)
        } else {
            None
        };

        let mut retries = 0;
        loop {
            for channel in list_channels(node, &node_id) {
                if let (Some(pubkey), Some(active)) = (
                    channel[remote_node_json_keyword].as_str(),
                    channel["active"].as_bool(),
                ) {
                    if pubkey.eq(target_node_id) && active {
                        // Wait a bit to avoid insufficient balance errors
                        sleep(Duration::from_secs(1));
                        return;
                    }
                }
            }
            sleep(Duration::from_millis(500));
            retries += 1;
            if retries >= 220 {
                panic!(
                    "Failed to create channel from {:?} to {}",
                    node, target_node_id
                );
            }
        }
    }

    fn list_channels(node: NodeInstance, node_id: &Option<String>) -> Vec<serde_json::Value> {
        match node {
            NodeInstance::NigiriCln => list_cln_channels(node, &node_id.clone().unwrap()),
            _ => list_lnd_channels(node),
        }
    }

    fn list_lnd_channels(node: NodeInstance) -> Vec<serde_json::Value> {
        let sub_cmd = &["listchannels"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            panic!("Command \"{:?}\" failed!", cmd);
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("Invalid json");

        json["channels"].as_array().unwrap().to_owned()
    }

    fn list_cln_channels(node: NodeInstance, self_node_id: &str) -> Vec<serde_json::Value> {
        let sub_cmd = &["listchannels"];
        let cmd = [get_node_prefix(node), sub_cmd].concat();

        let output = exec(cmd.as_slice());
        if !output.status.success() {
            panic!("Command \"{:?}\" failed!", cmd);
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("Invalid json");
        let channels = json["channels"].as_array().unwrap().to_owned();

        // CLN's listchannel command returns a somewhat surprising result:
        // - It returns all channels it knows, not only channels that belong to itself
        // - It returns all channels twice, once as an outgoing channel and once as an incoming channel
        //   Consequently, each 'owned' channel is returned once with its node id in the 'source' field
        //   and once with the its node id in the 'destination' field.
        channels
            .into_iter()
            .filter(|channel| self_node_id.eq(channel["source"].as_str().unwrap()))
            .collect()
    }

    fn lspd_home() -> String {
        [env!("PWD"), "/lspd"].concat()
    }

    fn rgs_home() -> String {
        [env!("PWD"), "/rgs"].concat()
    }
}
