use crate::print_events_handler::PrintEventsHandler;
use crate::setup::config::{get_testing_config, LOCAL_PERSISTENCE_PATH};

use uniffi_lipalightninglib::Config;
use uniffi_lipalightninglib::LightningNode;

use core::time::Duration;
use eel::errors::RuntimeErrorCode;
use std::fs;
use std::thread::sleep;

type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;

#[allow(dead_code)]
pub struct NodeHandle {
    config: Config,
}

#[allow(dead_code)]
impl NodeHandle {
    pub fn new() -> Self {
        let _ = fs::remove_dir_all(LOCAL_PERSISTENCE_PATH);
        fs::create_dir(LOCAL_PERSISTENCE_PATH).unwrap();

        let eel_config = get_testing_config();

        NodeHandle {
            config: Config {
                network: eel_config.network,
                seed: eel_config.seed.to_vec(),
                esplora_api_url: eel_config.esplora_api_url,
                rgs_url: eel_config.rgs_url,
                lsp_url: eel_config.lsp_url,
                lsp_token: eel_config.lsp_token,
                local_persistence_path: eel_config.local_persistence_path,
            },
        }
    }

    pub fn start(&self) -> Result<LightningNode> {
        let events_handler = PrintEventsHandler {};
        let node = LightningNode::new(self.config.clone(), Box::new(events_handler));

        // Wait for the the P2P background task to connect to the LSP
        sleep(Duration::from_millis(1500));

        node
    }
}
