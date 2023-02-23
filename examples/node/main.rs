mod cli;
mod hinter;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::LightningNode;
use uniffi_lipalightninglib::{Config, TzConfig};

use bitcoin::Network;
use eel::keys_manager::mnemonic_to_secret;
use log::info;
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

static BASE_DIR: &str = ".3l_node";
static LOG_FILE: &str = "logs.txt";

fn main() {
    // Create dir for node data persistence.
    fs::create_dir_all(BASE_DIR).unwrap();

    init_logger();
    info!("Logger initialized");

    let events = Box::new(PrintEventsHandler {});

    let seed = generate_seed();
    let config = Config {
        network: Network::Regtest,
        seed,
        fiat_currency: "EUR".to_string(),
        esplora_api_url: "http://localhost:30000".to_string(),
        rgs_url: "http://localhost:8080/snapshot/".to_string(),
        lsp_url: "http://127.0.0.1:6666".to_string(),
        lsp_token: "iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l".to_string(),
        local_persistence_path: BASE_DIR.to_string(),
        timezone_config: TzConfig {
            timezone_id: String::from("example_timezone_id"),
            timezone_utc_offset_secs: 1234,
        },
        graphql_url: get_backend_url(),
    };

    let node = LightningNode::new(config, events).unwrap();

    // Lauch CLI
    sleep(Duration::from_secs(1));
    cli::poll_for_user_input(&node, &format!("{BASE_DIR}/{LOG_FILE}"));
}

fn init_logger() {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}/{}", BASE_DIR, LOG_FILE))
        .unwrap();
    simplelog::CombinedLogger::init(vec![
        simplelog::TermLogger::new(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Auto,
        ),
        simplelog::WriteLogger::new(
            log::LevelFilter::Trace,
            simplelog::Config::default(),
            log_file,
        ),
    ])
    .unwrap();
}

fn generate_seed() -> Vec<u8> {
    let passphrase = "".to_string();
    let mnemonic = "kid rent scatter hire lonely deal simple olympic stool juice ketchup situate crouch taste stone badge act minute borrow mail venue lunar walk empower".to_string();
    let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
    let secret = mnemonic_to_secret(mnemonic, passphrase).unwrap();
    secret.seed
}

fn get_backend_url() -> String {
    env::var("GRAPHQL_API_URL").expect("GRAPHQL_API_URL environment variable is not set")
}
