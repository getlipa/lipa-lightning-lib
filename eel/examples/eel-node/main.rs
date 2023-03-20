mod cli;
mod file_storage;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use file_storage::FileStorage;

use crate::print_events_handler::PrintEventsHandler;

use bitcoin::Network;
use eel::config::{Config, TzConfig};
use eel::interfaces::{ExchangeRateProvider, RemoteStorage};
use eel::keys_manager::mnemonic_to_secret;
use eel::LightningNode;
use log::info;
use std::fs;
use std::thread::sleep;
use std::time::Duration;

static BASE_DIR: &str = ".eel_node";
static BASE_DIR_REMOTE: &str = ".eel_remote";
static LOG_FILE: &str = "logs.txt";

fn main() {
    // Create dir for node data persistence.
    fs::create_dir_all(BASE_DIR).unwrap();
    fs::create_dir_all(BASE_DIR_REMOTE).unwrap();

    init_logger();
    info!("Logger initialized");

    let remote_storage = Box::new(FileStorage::new(BASE_DIR_REMOTE));

    let events = Box::new(PrintEventsHandler {});
    let fiat_currency = "EUR".to_string();

    let seed_storage = FileStorage::new(BASE_DIR);
    let seed = read_or_generate_seed(&seed_storage);
    let config = Config {
        network: Network::Regtest,
        seed,
        fiat_currency,
        esplora_api_url: "http://localhost:30000".to_string(),
        rgs_url: "http://localhost:8080/snapshot/".to_string(),
        lsp_url: "http://127.0.0.1:6666".to_string(),
        lsp_token: "iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l".to_string(),
        local_persistence_path: BASE_DIR.to_string(),
        timezone_config: TzConfig {
            timezone_id: String::from("example_timezone_id"),
            timezone_utc_offset_secs: 1234,
        },
    };

    let exchange_rate_provider = Box::new(ExchangeRateProviderMock {});

    let node = LightningNode::new(config, remote_storage, events, exchange_rate_provider).unwrap();

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

fn read_or_generate_seed(storage: &FileStorage) -> [u8; 64] {
    let seed_file_name = "seed";
    let seed = match read_seed(storage, seed_file_name) {
        Some(seed) => seed,
        None => {
            info!("No existent seed found, generating a new one.");
            generate_seed(storage, seed_file_name)
        }
    };

    let mut seed_array = [0u8; 64];
    seed_array.copy_from_slice(&seed[..64]);
    seed_array
}

fn read_seed(storage: &FileStorage, seed_file_name: &str) -> Option<Vec<u8>> {
    match storage.get_object(".".to_string(), seed_file_name.to_string()) {
        Ok(seed) => Some(seed),
        Err(_) => None,
    }
}

fn generate_seed(storage: &FileStorage, seed_file_name: &str) -> Vec<u8> {
    let passphrase = "".to_string();
    let mnemonic = "kid rent scatter hire lonely deal simple olympic stool juice ketchup situate crouch taste stone badge act minute borrow mail venue lunar walk empower".to_string();
    let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
    let secret = mnemonic_to_secret(mnemonic, passphrase).unwrap();
    storage
        .put_object(
            ".".to_string(),
            seed_file_name.to_string(),
            secret.seed.clone(),
        )
        .unwrap();
    let mut mnemonic = secret.mnemonic.join(" ");
    mnemonic.push('\n');
    storage
        .put_object(
            ".".to_string(),
            "mnemonic".to_string(),
            mnemonic.into_bytes(),
        )
        .unwrap();
    secret.seed
}

struct ExchangeRateProviderMock;
impl ExchangeRateProvider for ExchangeRateProviderMock {
    fn query_exchange_rate(&self, _code: String) -> eel::errors::Result<u32> {
        Ok(1234)
    }
}
