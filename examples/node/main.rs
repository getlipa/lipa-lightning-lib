mod file_storage;

use file_storage::FileStorage;

use log::info;
use simplelog;
use std::fs;
use uniffi_lipalightninglib::callbacks::RedundantStorageCallback;
use uniffi_lipalightninglib::config::Config;
use uniffi_lipalightninglib::keys_manager::generate_secret_seed;
use uniffi_lipalightninglib::LightningNode;

static BASE_DIR: &str = ".ldk";

fn main() {
    // Create dir for node data persistence.
    fs::create_dir_all(BASE_DIR).unwrap();

    init_logger();
    info!("Logger initialized");

    let storage = Box::new(FileStorage::new(BASE_DIR));
    let secret_seed = get_secret_seed(&storage);
    let config = Config { secret_seed };
    let _node = LightningNode::new(config, storage).unwrap();
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

fn get_secret_seed(storage: &Box<FileStorage>) -> Vec<u8> {
    if storage.object_exists(".".to_string(), "secret_seed".to_string()) {
        return storage.get_object(".".to_string(), "secret_seed".to_string());
    }
    info!("No existent seed found, generating a new one");
    let new_seed = generate_secret_seed().unwrap();
    storage.put_object(".".to_string(), "secret_seed".to_string(), new_seed.clone());
    new_seed
}
