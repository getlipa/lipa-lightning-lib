mod cli;
mod hinter;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::LightningNode;
use uniffi_lipalightninglib::{Config, EnvironmentCode, TzConfig};

use eel::keys_manager::generate_secret;
use log::info;
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

static BASE_DIR: &str = ".3l_node";
static LOG_FILE: &str = "logs.txt";

fn main() {
    let environment = env::args().nth(1).unwrap_or("local".to_string());
    let base_dir = format!("{BASE_DIR}_{environment}");
    let environment = map_environment_code(&environment);

    // Create dir for node data persistence.
    fs::create_dir_all(&base_dir).unwrap();

    init_logger(&base_dir);
    info!("Logger initialized");

    let events = Box::new(PrintEventsHandler {});

    let seed = match fs::read(format!("{}/seed", base_dir)) {
        Ok(s) => s,
        Err(_) => {
            let seed = generate_seed();
            fs::write(format!("{}/seed", base_dir), &seed).unwrap();
            seed
        }
    };

    let config = Config {
        environment,
        seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: base_dir.clone(),
        timezone_config: TzConfig {
            timezone_id: String::from("Africa/Tunis"),
            timezone_utc_offset_secs: 1 * 60 * 60,
        },
    };

    let node = LightningNode::new(config, events).unwrap();

    // Lauch CLI
    sleep(Duration::from_secs(1));
    cli::poll_for_user_input(&node, &format!("{base_dir}/{LOG_FILE}"));
}

fn init_logger(path: &String) {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}/{}", path, LOG_FILE))
        .unwrap();

    let config = simplelog::ConfigBuilder::new()
        .add_filter_ignore_str("h2")
        .add_filter_ignore_str("hyper")
        .add_filter_ignore_str("mio")
        .add_filter_ignore_str("reqwest")
        .add_filter_ignore_str("rustls")
        .add_filter_ignore_str("rustyline")
        .add_filter_ignore_str("tokio_util")
        .add_filter_ignore_str("tonic")
        .add_filter_ignore_str("tower")
        .add_filter_ignore_str("tracing")
        .add_filter_ignore_str("ureq")
        .add_filter_ignore_str("want")
        .build();

    simplelog::CombinedLogger::init(vec![
        simplelog::TermLogger::new(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Auto,
        ),
        simplelog::WriteLogger::new(log::LevelFilter::Trace, config, log_file),
    ])
    .unwrap();
}

fn generate_seed() -> Vec<u8> {
    let passphrase = "".to_string();
    let secret = generate_secret(passphrase).unwrap();
    secret.seed
}

fn map_environment_code(code: &str) -> EnvironmentCode {
    match code {
        "local" => EnvironmentCode::Local,
        "dev" => EnvironmentCode::Dev,
        "stage" => EnvironmentCode::Stage,
        "prod" => EnvironmentCode::Prod,
        code => panic!("Unknown environment code: `{code}`"),
    }
}
