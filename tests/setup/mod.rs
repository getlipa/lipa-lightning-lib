use crate::print_events_handler::PrintEventsHandler;

use uniffi_lipalightninglib::{
    mnemonic_to_secret, AnalyticsConfig, BreezSdkConfig, Config, EventsCallback, LightningNode,
    MaxRoutingFeeConfig, ReceiveLimitsConfig, RemoteServicesConfig, RuntimeErrorCode, TzConfig,
};

use log::Level;
use std::fs;
use std::string::ToString;

type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;

const LOCAL_PERSISTENCE_PATH: &str = ".3l_local_test";

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum NodeType {
    Sender,
    Receiver,
}

#[macro_export]
macro_rules! wait_for_condition {
    ($cond:expr, $message_if_not_satisfied:expr, $attempts:expr, $sleep_duration:expr) => {
        (|| {
            for _ in 0..$attempts {
                if $cond {
                    return;
                }

                std::thread::sleep($sleep_duration);
            }

            let total_duration = $sleep_duration * $attempts;
            panic!("{} [after {total_duration:?}]", $message_if_not_satisfied);
        })()
    };
}

#[macro_export]
macro_rules! wait_for_condition_default {
    ($cond:expr, $message_if_not_satisfied:expr) => {
        let attempts = 1100;
        let sleep_duration = std::time::Duration::from_millis(100);
        wait_for_condition!($cond, $message_if_not_satisfied, attempts, sleep_duration)
    };
}

#[macro_export]
macro_rules! wait_for {
    ($cond:expr) => {
        let message_if_not_satisfied = format!("Failed to wait for `{}`", stringify!($cond));
        wait_for_condition_default!($cond, message_if_not_satisfied);
    };
}

#[allow(dead_code)]
pub fn start_node() -> Result<LightningNode> {
    start_specific_node(
        None,
        Box::new(PrintEventsHandler {}),
        true,
        Environment::Dev,
    )
}

#[allow(dead_code)] // Why is this needed?
pub enum Environment {
    Dev,
    Stage,
}

#[allow(dead_code)]
pub fn start_specific_node(
    node_type: Option<NodeType>,
    events_callback: Box<dyn EventsCallback>,
    reset_state: bool,
    environment: Environment,
) -> Result<LightningNode> {
    std::env::set_var("TESTING_TASK_PERIODS", "5");

    let node_name = match node_type {
        Some(NodeType::Sender) => "FUNDED_SENDER",
        Some(NodeType::Receiver) => "FUNDED_RECEIVER",
        None => "EMPTY",
    };

    let local_persistence_path = format!("{LOCAL_PERSISTENCE_PATH}/{node_name}");
    if reset_state {
        let _ = fs::remove_dir_all(local_persistence_path.clone());
        fs::create_dir_all(local_persistence_path.clone()).unwrap();
    }

    let mnemonic_key = format!("BREEZ_SDK_MNEMONIC_{node_name}");
    let mnemonic = std::env::var(&mnemonic_key)
        .unwrap_or_else(|_| panic!("Failed to load env var {mnemonic_key}"));
    let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
    let seed = mnemonic_to_secret(mnemonic, "".to_string()).unwrap().seed;

    let config = Config {
        seed,
        default_fiat_currency: "EUR".to_string(),
        local_persistence_path,
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
        remote_services_config: match environment {
            Environment::Dev => RemoteServicesConfig {
                backend_url: env!("BACKEND_COMPLETE_URL_DEV").to_string(),
                pocket_url: env!("POCKET_URL_DEV").to_string(),
                notification_webhook_base_url: env!("NOTIFICATION_WEBHOOK_URL_DEV").to_string(),
                notification_webhook_secret_hex: env!("NOTIFICATION_WEBHOOK_SECRET_DEV")
                    .to_string(),
                lipa_lightning_domain: env!("LIPA_LIGHTNING_DOMAIN_DEV").to_string(),
            },
            Environment::Stage => RemoteServicesConfig {
                backend_url: env!("BACKEND_COMPLETE_URL_STAGE").to_string(),
                pocket_url: env!("POCKET_URL_STAGE").to_string(),
                notification_webhook_base_url: env!("NOTIFICATION_WEBHOOK_URL_STAGE").to_string(),
                notification_webhook_secret_hex: env!("NOTIFICATION_WEBHOOK_SECRET_STAGE")
                    .to_string(),
                lipa_lightning_domain: env!("LIPA_LIGHTNING_DOMAIN_STAGE").to_string(),
            },
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

    let node = LightningNode::new(config, events_callback)?;
    node.set_analytics_config(AnalyticsConfig::Disabled)?; // tests produce misleading noise

    Ok(node)
}
