mod file_storage;

use file_storage::FileStorage;

use bitcoin::Network;
use log::info;
use std::env;
use std::fs;
use uniffi_lipalightninglib::callbacks::RedundantStorageCallback;
use uniffi_lipalightninglib::config::{Config, NodeAddress};
use uniffi_lipalightninglib::keys_manager::generate_secret;
use uniffi_lipalightninglib::LightningNode;

static BASE_DIR: &str = ".ldk";

fn main() {
    dotenv::from_path("examples/node/.env").unwrap();

    // Create dir for node data persistence.
    fs::create_dir_all(BASE_DIR).unwrap();

    init_logger();
    info!("Logger initialized");

    let storage = Box::new(FileStorage::new(BASE_DIR));
    let seed = read_or_generate_seed(&storage);
    let config = Config {
        network: Network::Regtest,
        seed,
        esplora_api_url: "http://localhost:30000".to_string(),
        lsp_node: NodeAddress {
            pub_key: env::var("LSP_NODE_PUB_KEY").unwrap(),
            address: env::var("LSP_NODE_ADDRESS").unwrap(),
        },
    };

    let _node = LightningNode::new(&config, storage).unwrap();
}

fn init_logger() {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(BASE_DIR.to_string() + "/logs.txt")
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

fn read_or_generate_seed(storage: &Box<FileStorage>) -> Vec<u8> {
    let seed_file_name = "seed";
    if storage.object_exists(".".to_string(), seed_file_name.to_string()) {
        return storage.get_object(".".to_string(), seed_file_name.to_string());
    }
    info!("No existent seed found, generating a new one.");
    let passphrase = "".to_string();
    let secret = generate_secret(passphrase).unwrap();
    storage.put_object(
        ".".to_string(),
        seed_file_name.to_string(),
        secret.seed.clone(),
    );
    let mut mnemonic = secret.mnemonic.join(" ");
    mnemonic.push('\n');
    storage.put_object(
        ".".to_string(),
        "mnemonic".to_string(),
        mnemonic.into_bytes(),
    );
    secret.seed
}
