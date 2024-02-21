use std::env;
use uniffi_lipalightninglib::{
    handle_notification, mnemonic_to_secret, Config, EnvironmentCode, TzConfig,
};

static BASE_DIR: &str = ".3l_node";

fn main() {
    let hash = env::args().nth(1).expect("A payment hash must be provided");
    let environment = env::args().nth(2).unwrap_or("local".to_string());

    println!("Starting a handle_notification test run.");
    println!("Environment: {environment}");
    println!("Payment hash we are looking for: {hash}");
    println!();

    let base_dir = format!("{BASE_DIR}_{environment}");

    let environment = map_environment_code(&environment);

    let seed = read_seed_from_env();

    let config = Config {
        environment,
        seed,
        fiat_currency: "EUR".to_string(),
        local_persistence_path: base_dir.clone(),
        timezone_config: TzConfig {
            timezone_id: String::from("Africa/Tunis"),
            timezone_utc_offset_secs: 60 * 60,
        },
        enable_file_logging: true,
    };

    let notification_payload = format!(
        "{{
         \"template\": \"payment_received\",
         \"data\": {{
          \"payment_hash\": \"{hash}\"
         }}
        }}"
    );

    let action = handle_notification(config, notification_payload).unwrap();

    println!("The recommended action is {action:?}");
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
