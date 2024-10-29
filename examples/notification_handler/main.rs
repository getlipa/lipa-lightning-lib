mod environment;
mod hinter;

use crate::environment::{Environment, EnvironmentCode};
use crate::hinter::{CommandHint, CommandHinter};
use anyhow::{anyhow, Result};
use colored::Colorize;
use lazy_static::lazy_static;
use log::Level;
use rustyline::config::Builder;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{CompletionType, Editor};
use std::collections::HashSet;
use std::env;
use std::time::Duration;
use uniffi_lipalightninglib::{
    handle_notification, mnemonic_to_secret, BreezSdkConfig, Config, MaxRoutingFeeConfig,
    NotificationToggles, ReceiveLimitsConfig, RemoteServicesConfig, TzConfig,
};

static BASE_DIR: &str = ".3l_node";

lazy_static! {
    static ref ENVIRONMENT: String = env::args().nth(1).unwrap_or("local".to_string());
}

fn main() {
    let prompt = "3L Notification Handler ϟ ".bold().blue().to_string();
    let mut rl = setup_editor();
    loop {
        let line = match rl.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                println!("{}", e.to_string().red());
                continue;
            }
        };

        let mut words = line.split_whitespace();
        if let Some(word) = words.next() {
            match word {
                "h" | "help" => help(),
                "payment_received" => {
                    if let Err(message) = start_payment_received(&mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "address_txs_confirmed" => {
                    if let Err(message) = start_address_txs_confirmed(&mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "lnurl_pay_request" => {
                    if let Err(message) = start_lnurl_pay_request(&mut words) {
                        println!("{}", format!("{message:#}").red());
                    }
                }
                "stop" => {
                    break;
                }
                _ => println!(
                    "{}",
                    "Unknown command. See \"help\" for available commands.".red()
                ),
            }
        }
    }
}

fn setup_editor() -> Editor<CommandHinter, DefaultHistory> {
    let config = Builder::new()
        .auto_add_history(true)
        .completion_type(CompletionType::List)
        .build();

    let mut hints = HashSet::new();
    hints.insert(CommandHint::new(
        "payment_received <hash>",
        "payment_received ",
    ));
    hints.insert(CommandHint::new(
        "address_txs_confirmed <address>",
        "address_txs_confirmed ",
    ));
    hints.insert(CommandHint::new(
        "lnurl_pay_request <amount_msat> <id> <recipient> [payer_comment]",
        "lnurl_pay_request ",
    ));
    hints.insert(CommandHint::new("stop", "stop"));
    let hinter = CommandHinter { hints };

    let mut rl = Editor::<CommandHinter, DefaultHistory>::with_config(config).unwrap();
    rl.set_helper(Some(hinter));
    rl
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

fn get_config() -> Config {
    let base_dir = format!("{BASE_DIR}_{}", ENVIRONMENT.as_str());

    let environment_code = map_environment_code(ENVIRONMENT.as_str());
    let environment = Environment::load(environment_code);

    let seed = read_seed_from_env();

    Config {
        seed,
        default_fiat_currency: "EUR".to_string(),
        local_persistence_path: base_dir.clone(),
        timezone_config: TzConfig {
            timezone_id: String::from("Africa/Tunis"),
            timezone_utc_offset_secs: 60 * 60,
        },
        file_logging_level: Some(Level::Debug),
        phone_number_allowed_countries_iso_3166_1_alpha_2: vec![
            "AT".to_string(),
            "CH".to_string(),
            "DE".to_string(),
        ],
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
    }
}

fn help() {
    println!("  h | help");
    println!("  payment_received <hash>");
    println!("  address_txs_confirmed <address>");
    println!("  lnurl_pay_request <amount_msat> <id> <recipient> [payer_comment]");
    println!("  stop");
}

fn start_payment_received(words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let hash = words.next().ok_or(anyhow!("Payment hash is required"))?;

    println!("Starting a handle_notification(payment_received) test run.");
    println!("Environment: {}", ENVIRONMENT.as_str());
    println!("Payment hash we are looking for: {hash}");
    println!();

    let config = get_config();

    let notification_payload = format!(
        "{{
         \"template\": \"payment_received\",
         \"data\": {{
          \"payment_hash\": \"{hash}\"
         }}
        }}"
    );

    let notification = handle_notification(
        config,
        notification_payload,
        NotificationToggles {
            payment_received_is_enabled: true,
            address_txs_confirmed_is_enabled: true,
            lnurl_pay_request_is_enabled: true,
        },
        Duration::from_secs(60),
    )
    .unwrap();

    println!("The returned notification is {notification:?}");

    Ok(())
}

fn start_address_txs_confirmed(words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let address = words.next().ok_or(anyhow!("Address is required"))?;

    println!("Starting a handle_notification(address_txs_confirmed) test run.");
    println!("Environment: {}", ENVIRONMENT.as_str());
    println!("Swap address we are interested in: {address}");
    println!();

    let config = get_config();

    let notification_payload = format!(
        "{{
         \"template\": \"address_txs_confirmed\",
         \"data\": {{
          \"payment_hash\": \"{address}\"
         }}
        }}"
    );

    let notification = handle_notification(
        config,
        notification_payload,
        NotificationToggles {
            payment_received_is_enabled: true,
            address_txs_confirmed_is_enabled: true,
            lnurl_pay_request_is_enabled: true,
        },
        Duration::from_secs(60),
    )
    .unwrap();

    println!("The returned notification is {notification:?}");

    Ok(())
}

fn start_lnurl_pay_request(words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let amount_msat: u64 = words
        .next()
        .ok_or(anyhow!("amount_msat is required"))?
        .parse()?;
    let id = words.next().ok_or(anyhow!("id is required"))?;
    let recipient = words.next().ok_or(anyhow!("recipient is required"))?;
    let payer_comment = words.collect::<Vec<_>>().join(" ");

    println!("Starting a handle_notification(lnurl_pay_request) test run.");
    println!("Environment: {}", ENVIRONMENT.as_str());
    println!("Amount (msat): {amount_msat}");
    println!("ID: {id}");
    println!("Recipient: {recipient}");
    println!("Payer Comment: {payer_comment}");
    println!();

    let config = get_config();

    let notification_payload = format!(
        "{{
         \"template\": \"lnurl_pay_request\",
         \"data\": {{
          \"amount_msat\": {amount_msat},
          \"recipient\": \"{recipient}\",
          \"payer_comment\": \"{payer_comment}\",
          \"id\": \"{id}\"
         }}
        }}"
    );

    let notification = handle_notification(
        config,
        notification_payload,
        NotificationToggles {
            payment_received_is_enabled: true,
            address_txs_confirmed_is_enabled: true,
            lnurl_pay_request_is_enabled: true,
        },
        Duration::from_secs(60),
    )
    .unwrap();

    println!("The returned notification is {notification:?}");

    Ok(())
}
