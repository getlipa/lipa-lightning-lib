use crate::print_events_handler::PrintEventsHandler;
use crate::setup_env::config::{get_testing_config, LOCAL_PERSISTENCE_PATH};

use uniffi_lipalightninglib::LightningNode;
use uniffi_lipalightninglib::{recover_lightning_node, Config};

use crate::wait_for_eq;
use core::time::Duration;
use eel::config::TzConfig;
use eel::errors::RuntimeErrorCode;
use std::env;
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
        Self::reset_state();

        let eel_config = get_testing_config();

        NodeHandle {
            config: Config {
                network: eel_config.network,
                seed: eel_config.seed.to_vec(),
                fiat_currency: "EUR".to_string(),
                esplora_api_url: eel_config.esplora_api_url,
                rgs_url: eel_config.rgs_url,
                lsp_url: eel_config.lsp_url,
                lsp_token: eel_config.lsp_token,
                local_persistence_path: eel_config.local_persistence_path,
                timezone_config: TzConfig {
                    timezone_id: String::from("int_test_timezone_id"),
                    timezone_utc_offset_secs: 1234,
                },
                graphql_url: get_backend_url(),
                backend_health_url: get_backend_health_url(),
            },
        }
    }

    pub fn start(&self) -> Result<LightningNode> {
        let events_handler = PrintEventsHandler {};
        let node = LightningNode::new(self.config.clone(), Box::new(events_handler))?;

        // Wait for the the P2P background task to connect to the LSP
        wait_for_eq!(node.get_node_info().num_peers, 1);

        Ok(node)
    }

    pub fn reset_state() {
        let _ = fs::remove_dir_all(LOCAL_PERSISTENCE_PATH);
        fs::create_dir(LOCAL_PERSISTENCE_PATH).unwrap();
    }

    pub fn recover(&self) -> eel::errors::Result<()> {
        recover_lightning_node(
            self.config.seed.to_vec(),
            self.config.local_persistence_path.clone(),
            self.config.graphql_url.clone(),
            self.config.backend_health_url.clone(),
        )
    }
}

fn get_backend_url() -> String {
    format!("{}/v1/graphql", get_base_url())
}

fn get_backend_health_url() -> String {
    format!("{}/healthz", get_base_url())
}

fn get_base_url() -> String {
    let base_url =
        env::var("BACKEND_BASE_URL").expect("BACKEND_BASE_URL environment variable is not set");
    sanitize_backend_base_url(&base_url);

    base_url
}

fn sanitize_backend_base_url(url: &str) {
    if url.contains("healthz") || url.contains("graphql") {
        panic!("Make sure the BACKEND_BASE_URL environment variable does not include any path like '/v1/graphql'. It's a base URL.");
    }
}
