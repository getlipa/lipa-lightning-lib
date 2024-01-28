mod cli;
mod hinter;
mod overview;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::{mnemonic_to_secret, recover_lightning_node, LightningNode};
use uniffi_lipalightninglib::{Config, EnvironmentCode, TzConfig};

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

static BASE_DIR: &str = ".3l_node";
static LOG_FILE: &str = "logs.txt";

fn main() {
    let environment = env::args().nth(1).unwrap_or("local".to_string());
    let base_dir = format!("{BASE_DIR}_{environment}");

    #[cfg(feature = "breez-sdk-mock")]
    let base_dir = format!("{base_dir}_mocked");
    #[cfg(feature = "breez-sdk-mock")]
    fs::remove_dir_all(&base_dir).unwrap();

    let environment = map_environment_code(&environment);

    // Create dir for node data persistence.
    fs::create_dir_all(&base_dir).unwrap();

    let events = Box::new(PrintEventsHandler {});

    let seed = read_seed_from_env();

    if Path::new(&base_dir)
        .read_dir()
        .is_ok_and(|mut d| d.next().is_none())
    {
        recover_lightning_node(environment, seed.clone(), base_dir.clone(), true).unwrap();
    }

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

    // Launch CLI
    sleep(Duration::from_secs(1));
    cli::poll_for_user_input(&node, &format!("{base_dir}/logs/{LOG_FILE}"));
}

fn read_seed_from_env() -> Vec<u8> {
    let mnemonic = env!("BREEZ_SDK_MNEMONIC");
    let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
    mnemonic_to_secret(mnemonic, "".to_string()).unwrap().seed
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
