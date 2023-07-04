mod cli;
mod hinter;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::LightningNode;
use uniffi_lipalightninglib::{Config, EnvironmentCode, TzConfig};

use eel::keys_manager::{generate_secret, mnemonic_to_secret};
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

    let events = Box::new(PrintEventsHandler {});

    let seed = read_or_generate_seed(&base_dir);

    let config = Config {
        environment,
        seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: base_dir.clone(),
        timezone_config: TzConfig {
            timezone_id: String::from("Africa/Tunis"),
            timezone_utc_offset_secs: 1 * 60 * 60,
        },
        enable_file_logging: true,
    };

    let node = LightningNode::new(config, events).unwrap();

    // Lauch CLI
    sleep(Duration::from_secs(1));
    cli::poll_for_user_input(&node, &format!("{base_dir}/{LOG_FILE}"));
}

fn read_or_generate_seed(base_dir: &str) -> Vec<u8> {
    let passphrase = "".to_string();
    let filename = format!("{base_dir}/recovery_phrase");
    match fs::read(filename.clone()) {
        Ok(mnemonic) => {
            let mnemonic = std::str::from_utf8(&mnemonic).unwrap();
            let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
            mnemonic_to_secret(mnemonic, passphrase).unwrap().seed
        }
        Err(_) => {
            let secret = generate_secret(passphrase).unwrap();
            let recovery_phrase = secret.mnemonic.join(" ");
            fs::write(filename, &recovery_phrase).unwrap();
            secret.seed
        }
    }
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
