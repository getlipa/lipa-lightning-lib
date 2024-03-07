use crate::print_events_handler::PrintEventsHandler;
use crate::wait_for;

use uniffi_lipalightninglib::{mnemonic_to_secret, Config, TzConfig};
use uniffi_lipalightninglib::{LightningNode, RuntimeErrorCode};

use std::fs;
use std::string::ToString;

type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;

const LOCAL_PERSISTENCE_PATH: &str = ".3l_local_test";

#[macro_export]
macro_rules! wait_for_condition {
    ($cond:expr, $message_if_not_satisfied:expr, $attempts:expr, $sleep_duration:expr) => {
        (|| {
            for _ in 0..$attempts {
                if $cond {
                    return;
                }

                std::thread::sleep($sleep_duration);
            }

            let total_duration = $sleep_duration * $attempts;
            panic!("{} [after {total_duration:?}]", $message_if_not_satisfied);
        })()
    };
}

#[macro_export]
macro_rules! wait_for_condition_default {
    ($cond:expr, $message_if_not_satisfied:expr) => {
        let attempts = 1100;
        let sleep_duration = std::time::Duration::from_millis(100);
        wait_for_condition!($cond, $message_if_not_satisfied, attempts, sleep_duration)
    };
}

#[macro_export]
macro_rules! wait_for {
    ($cond:expr) => {
        let message_if_not_satisfied = format!("Failed to wait for `{}`", stringify!($cond));
        wait_for_condition_default!($cond, message_if_not_satisfied);
    };
}

#[allow(dead_code)]
pub fn start_alice() -> Result<LightningNode> {
    start_node("ALICE")
}

#[allow(dead_code)]
pub fn start_bob() -> Result<LightningNode> {
    start_node("BOB")
}

fn start_node(node_name: &str) -> Result<LightningNode> {
    std::env::set_var("TESTING_TASK_PERIODS", "5");

    let local_persistence_path = format!("{LOCAL_PERSISTENCE_PATH}/{node_name}");
    let _ = fs::remove_dir_all(local_persistence_path.clone());
    fs::create_dir_all(local_persistence_path.clone()).unwrap();

    let mnemonic_key = format!("BREEZ_SDK_MNEMONIC_{node_name}");
    let mnemonic = std::env::var(mnemonic_key).unwrap();
    let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
    let seed = mnemonic_to_secret(mnemonic, "".to_string()).unwrap().seed;

    let config = Config {
        environment: uniffi_lipalightninglib::EnvironmentCode::Dev,
        seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path,
        timezone_config: TzConfig {
            timezone_id: String::from("int_test_timezone_id"),
            timezone_utc_offset_secs: 1234,
        },
        enable_file_logging: false,
    };

    let events_handler = PrintEventsHandler {};
    let node = LightningNode::new(config, Box::new(events_handler))?;

    // Wait for the P2P background task to connect to the LSP
    wait_for!(!node.get_node_info().unwrap().peers.is_empty());

    Ok(node)
}
