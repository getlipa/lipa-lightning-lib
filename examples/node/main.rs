mod cli;
mod environment;
mod hinter;
mod overview;
#[path = "../../tests/print_events_handler/mod.rs"]
mod print_events_handler;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::{
    mnemonic_to_secret, recover_lightning_node, BreezSdkConfig, LightningNode, MaxRoutingFeeConfig,
    ReceiveLimitsConfig, RemoteServicesConfig,
};
use uniffi_lipalightninglib::{Config, TzConfig};

use crate::environment::{Environment, EnvironmentCode};
use log::Level;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

static BASE_DIR: &str = ".3l_node";
static LOG_FILE: &str = "logs.txt";

fn main() {
    let environment = env::args().nth(1).unwrap_or("local".to_string());
    let base_dir = format!("{BASE_DIR}_{environment}");

    #[cfg(feature = "mock-deps")]
    let base_dir = format!("{base_dir}_mocked");
    #[cfg(feature = "mock-deps")]
    if let Err(err) = fs::remove_dir_all(&base_dir) {
        log::warn!("Error deleting directory: {}", err);
    }

    let environment_code = map_environment_code(&environment);
    let environment = Environment::load(environment_code);

    // Create dir for node data persistence.
    fs::create_dir_all(&base_dir).unwrap();

    let events = Box::new(PrintEventsHandler {});

    let seed = read_seed_from_env();

    if Path::new(&base_dir)
        .read_dir()
        .is_ok_and(|mut d| d.next().is_none())
    {
        recover_lightning_node(
            environment.backend_url.clone(),
            seed.clone(),
            base_dir.clone(),
            Some(Level::Debug),
        )
        .unwrap();
    }

    let config = Config {
        seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: base_dir.clone(),
        timezone_config: TzConfig {
            timezone_id: String::from("Africa/Tunis"),
            timezone_utc_offset_secs: 60 * 60,
        },
        file_logging_level: Some(Level::Debug),
        phone_number_allowed_countries_iso_3166_1_alpha_2: ["AT", "CH", "DE"]
            .map(String::from)
            .to_vec(),
        remote_services_config: RemoteServicesConfig {
            backend_url: environment.backend_url.clone(),
            pocket_url: environment.pocket_url.clone(),
            notification_webhook_base_url: environment.notification_webhook_base_url.clone(),
            notification_webhook_secret_hex: environment.notification_webhook_secret_hex.clone(),
            lipa_lightning_domain: environment.lipa_lightning_domain,
        },
        breez_sdk_config: BreezSdkConfig {
            breez_sdk_api_key: env!("BREEZ_SDK_API_KEY").to_string(),
            breez_sdk_partner_certificate: env!("BREEZ_SDK_PARTNER_CERTIFICATE").to_string(),
            breez_sdk_partner_key: env!("BREEZ_SDK_PARTNER_KEY").to_string(),
        },
        max_routing_fee_config: MaxRoutingFeeConfig {
            max_routing_fee_permyriad: 150,
            max_routing_fee_exempt_fee_sats: 21,
        },
        receive_limits_config: ReceiveLimitsConfig {
            max_receive_amount_sat: 1_000_000,
            min_receive_channel_open_fee_multiplier: 2.0,
        },
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
