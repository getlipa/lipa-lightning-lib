mod print_events_handler;
mod setup;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::{generate_secret, Config, TzConfig};
use uniffi_lipalightninglib::{BreezLightningNode, LightningNode};

use serial_test::file_serial;
use std::fs;
use std::string::ToString;

const LOCAL_PERSISTENCE_PATH: &str = ".3l_local_test";

#[test]
#[ignore = "We do not want to register a ton of nodes"]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_register_node() {
    std::env::set_var("TESTING_TASK_PERIODS", "5");
    let _ = fs::remove_dir_all(LOCAL_PERSISTENCE_PATH);
    fs::create_dir(LOCAL_PERSISTENCE_PATH).unwrap();

    let secret = generate_secret(String::new()).unwrap();
    let mnemonic = secret.mnemonic.join(" ");
    println!("Mnemonic: {mnemonic}");

    let config = Config {
        environment: uniffi_lipalightninglib::EnvironmentCode::Local,
        seed: secret.seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: LOCAL_PERSISTENCE_PATH.to_string(),
        timezone_config: TzConfig {
            timezone_id: String::from("int_test_timezone_id"),
            timezone_utc_offset_secs: 1234,
        },
        enable_file_logging: false,
    };

    let events_handler = PrintEventsHandler {};
    let node = BreezLightningNode::new(config, Box::new(events_handler)).unwrap();
    // Wait for the the P2P background task to connect to the LSP
    wait_for!(!node.get_node_info().unwrap().peers.is_empty());
}
