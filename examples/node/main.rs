use simplelog;
use std::fs;

fn main() {
    // Create dir for node data persistence
    fs::create_dir_all(".ldk").unwrap();

    init_logger();
    println!("Logger initialized");
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
