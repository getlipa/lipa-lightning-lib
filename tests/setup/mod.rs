use crate::print_events_handler::PrintEventsHandler;
use crate::wait_for;

use uniffi_lipalightninglib::{mnemonic_to_secret, recover_lightning_node, Config, TzConfig};
use uniffi_lipalightninglib::{LightningNode, RuntimeErrorCode};

use std::fs;
use std::string::ToString;

type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;

pub struct NodeHandle {
    config: Config,
}

const LOCAL_PERSISTENCE_PATH: &str = ".3l_local_test";

#[macro_export]
macro_rules! wait_for_condition {
    ($cond:expr, $message_if_not_satisfied:expr) => {
        (|| {
            let attempts = 1100;
            let sleep_duration = std::time::Duration::from_millis(100);
            for _ in 0..attempts {
                if $cond {
                    return;
                }

                std::thread::sleep(sleep_duration);
            }

            let total_duration = sleep_duration * attempts;
            panic!("{} [after {total_duration:?}]", $message_if_not_satisfied);
        })();
    };
}

#[macro_export]
macro_rules! wait_for {
    ($cond:expr) => {
        let message_if_not_satisfied = format!("Failed to wait for `{}`", stringify!($cond));
        wait_for_condition!($cond, message_if_not_satisfied);
    };
}

#[allow(dead_code)]
impl NodeHandle {
    pub fn new() -> Self {
        std::env::set_var("TESTING_TASK_PERIODS", "5");

        Self::reset_state();

        let mnemonic = std::env::var("BREEZ_SDK_MNEMONIC").unwrap();
        let mnemonic = mnemonic.split_whitespace().map(String::from).collect();

        NodeHandle {
            config: Config {
                environment: uniffi_lipalightninglib::EnvironmentCode::Local,
                seed: mnemonic_to_secret(mnemonic, "".to_string()).unwrap().seed,
                fiat_currency: "EUR".to_string(),
                local_persistence_path: LOCAL_PERSISTENCE_PATH.to_string(),
                timezone_config: TzConfig {
                    timezone_id: String::from("int_test_timezone_id"),
                    timezone_utc_offset_secs: 1234,
                },
                enable_file_logging: false,
            },
        }
    }

    pub fn start(&self) -> Result<LightningNode> {
        let events_handler = PrintEventsHandler {};
        let node = LightningNode::new(self.config.clone(), Box::new(events_handler))?;

        // Wait for the the P2P background task to connect to the LSP
        wait_for!(!node.get_node_info().unwrap().peers.is_empty());

        Ok(node)
    }

    pub fn reset_state() {
        let _ = fs::remove_dir_all(LOCAL_PERSISTENCE_PATH);
        fs::create_dir(LOCAL_PERSISTENCE_PATH).unwrap();
    }

    pub fn recover(&self) -> Result<()> {
        recover_lightning_node(
            self.config.environment,
            self.config.seed.to_vec(),
            self.config.local_persistence_path.clone(),
            false,
        )
    }
}
