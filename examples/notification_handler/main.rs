mod hinter;

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
use uniffi_lipalightninglib::{
    handle_notification, mnemonic_to_secret, Config, EnvironmentCode, TzConfig,
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

    let environment = map_environment_code(ENVIRONMENT.as_str());

    let seed = read_seed_from_env();

    Config {
        environment,
        seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: base_dir.clone(),
        timezone_config: TzConfig {
            timezone_id: String::from("Africa/Tunis"),
            timezone_utc_offset_secs: 60 * 60,
        },
        file_logging_level: Some(Level::Debug),
    }
}

fn help() {
    println!("  h | help");
    println!("  payment_received <hash>");
    println!("  address_txs_confirmed <address>");
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

    let notification = handle_notification(config, notification_payload).unwrap();

    println!("The returned notification is {notification:?}");

    Ok(())
}

fn start_address_txs_confirmed(words: &mut dyn Iterator<Item = &str>) -> Result<()> {
    let address = words.next().ok_or(anyhow!("Address is required"))?;
    let environment = words.next().unwrap_or("local");

    println!("Starting a handle_notification(address_txs_confirmed) test run.");
    println!("Environment: {environment}");
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

    let notification = handle_notification(config, notification_payload).unwrap();

    println!("The returned notification is {notification:?}");

    Ok(())
}
