mod cli;
mod file_storage;
#[path = "../../tests/lsp_client/mod.rs"]
mod lsp_client;

pub mod lspd {
    tonic::include_proto!("lspd");
}

use file_storage::FileStorage;
use lsp_client::LspClient;

use bitcoin::hashes::hex::ToHex;
use bitcoin::Network;
use log::info;
use lspd::ChannelInformationReply;
use prost::Message;
use std::fs;
use std::thread::sleep;
use std::time::Duration;
use uniffi_lipalightninglib::callbacks::{LspCallback, RedundantStorageCallback};
use uniffi_lipalightninglib::config::{Config, NodeAddress};
use uniffi_lipalightninglib::keys_manager::mnemonic_to_secret;
use uniffi_lipalightninglib::LightningNode;

static BASE_DIR: &str = ".ldk";
static LOG_FILE: &str = "logs.txt";

fn main() {
    // Create dir for node data persistence.
    fs::create_dir_all(BASE_DIR).unwrap();

    init_logger();
    info!("Logger initialized");

    let lsp_address = "http://127.0.0.1:6666".to_string();
    info!("Contacting lsp at {} ...", lsp_address);
    let lsp_auth_token =
        "iQUvOsdk4ognKshZB/CKN2vScksLhW8i13vTO+8SPvcyWJ+fHi8OLgUEvW1N3k2l".to_string();
    let lsp_client = Box::new(LspClient::build(lsp_address, lsp_auth_token));
    let lsp_info = lsp_client.channel_information().unwrap();
    let lsp_info = ChannelInformationReply::decode(&*lsp_info).unwrap();
    let ln_node_address = NodeAddress {
        pub_key: lsp_info.pubkey,
        address: lsp_info.host,
    };
    info!("Lsp pubkey: {}", lsp_info.lsp_pubkey.to_hex());
    info!("LN node {:?}", ln_node_address);

    let storage = Box::new(FileStorage::new(BASE_DIR));
    let seed = read_or_generate_seed(&storage);
    let config = Config {
        network: Network::Regtest,
        seed,
        esplora_api_url: "http://localhost:30000".to_string(),
        lsp_node: ln_node_address,
    };

    let node = LightningNode::new(&config, storage, lsp_client).unwrap();

    // Lauch CLI
    sleep(Duration::from_secs(1));
    cli::poll_for_user_input(&node, &format!("{}/{}", BASE_DIR, LOG_FILE));
}

fn init_logger() {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}/{}", BASE_DIR, LOG_FILE))
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

fn read_or_generate_seed(storage: &FileStorage) -> Vec<u8> {
    let seed_file_name = "seed";
    if storage.object_exists(".".to_string(), seed_file_name.to_string()) {
        return storage.get_object(".".to_string(), seed_file_name.to_string());
    }
    info!("No existent seed found, generating a new one.");
    let passphrase = "".to_string();
    let mnemonic = "kid rent scatter hire lonely deal simple olympic stool juice ketchup situate crouch taste stone badge act minute borrow mail venue lunar walk empower".to_string();
    let mnemonic = mnemonic.split_whitespace().map(String::from).collect();
    let secret = mnemonic_to_secret(mnemonic, passphrase).unwrap();
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
