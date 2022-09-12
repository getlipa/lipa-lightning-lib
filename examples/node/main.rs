mod file_storage;

use file_storage::FileStorage;
use simplelog;
use std::fs;
use uniffi_lipalightninglib::LightningNode;

static BASE_DIR: &str = ".ldk";

fn main() {
    // Create dir for node data persistence
    fs::create_dir_all(BASE_DIR).unwrap();

    init_logger();
    println!("Logger initialized");

    let storage = Box::new(FileStorage::new(BASE_DIR));
    let _node = LightningNode::new(storage);
}

fn init_logger() {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(".ldk/logs.txt")
        .unwrap();
    simplelog::CombinedLogger::init(vec![
        simplelog::TermLogger::new(
            log::LevelFilter::Warn,
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
