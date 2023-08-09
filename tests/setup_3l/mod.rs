use crate::print_events_handler::PrintEventsHandler;
use crate::setup_env::config::{get_testing_config, LOCAL_PERSISTENCE_PATH};

use uniffi_lipalightninglib::{recover_lightning_node, Config};
use uniffi_lipalightninglib::{LightningNode, RuntimeErrorCode};

use crate::wait_for;
use eel::config::TzConfig;
use std::fs;

type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;

#[allow(dead_code)]
pub struct NodeHandle {
    config: Config,
}

#[allow(dead_code)]
impl NodeHandle {
    pub fn new() -> Self {
        std::env::set_var("TESTING_TASK_PERIODS", "5");

        Self::reset_state();

        let eel_config = get_testing_config();

        NodeHandle {
            config: Config {
                environment: uniffi_lipalightninglib::EnvironmentCode::Local,
                seed: eel_config.seed.to_vec(),
                fiat_currency: "EUR".to_string(),
                local_persistence_path: eel_config.local_persistence_path,
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
        wait_for!(!node.get_node_info().peers.is_empty());

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
