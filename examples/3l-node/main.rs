mod cli;
mod hinter;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::LightningNode;
use uniffi_lipalightninglib::{Config, TzConfig};

use bitcoin::Network;
use eel::keys_manager::generate_secret;
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

    let mut args = env::args();
    let _ = args.next();
    let fiat_currency = args.next().unwrap_or("EUR".to_string());

    let seed = match fs::read(format!("{}/seed", BASE_DIR)) {
        Ok(s) => s,
        Err(_) => {
            let seed = generate_seed();
            fs::write(format!("{}/seed", BASE_DIR), &seed).unwrap();
            seed
        }
    };

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
            timezone_id: String::from("Europe/Zurich"),
            timezone_utc_offset_secs: 1 * 60 * 60,
        },
        graphql_url: get_backend_url(),
        backend_health_url: get_backend_health_url(),
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
    let secret = generate_secret(passphrase).unwrap();
    secret.seed
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
