mod print_events_handler;
mod setup;

use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::{generate_secret, Config, ReceiveLimitsConfig, TzConfig};
use uniffi_lipalightninglib::{
    BreezSdkConfig, LightningNode, MaxRoutingFeeConfig, RemoteServicesConfig,
};

use log::Level;
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
        seed: secret.seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: LOCAL_PERSISTENCE_PATH.to_string(),
        timezone_config: TzConfig {
            timezone_id: String::from("int_test_timezone_id"),
            timezone_utc_offset_secs: 1234,
        },
        file_logging_level: Some(Level::Debug),
        phone_number_allowed_countries_iso_3166_1_alpha_2: vec![
            "AT".to_string(),
            "CH".to_string(),
            "DE".to_string(),
        ],
        remote_services_config: RemoteServicesConfig {
            backend_url: env!("BACKEND_URL_LOCAL").to_string(),
            pocket_url: env!("POCKET_URL_LOCAL").to_string(),
            notification_webhook_base_url: env!("NOTIFICATION_WEBHOOK_URL_LOCAL").to_string(),
            notification_webhook_secret_hex: env!("NOTIFICATION_WEBHOOK_SECRET_LOCAL").to_string(),
            lipa_lightning_domain: env!("LIPA_LIGHTNING_DOMAIN_LOCAL").to_string(),
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

    let events_handler = PrintEventsHandler {};
    let node = LightningNode::new(config, Box::new(events_handler)).unwrap();
    // Wait for the the P2P background task to connect to the LSP
    wait_for!(!node.get_node_info().unwrap().peers.is_empty());
}
