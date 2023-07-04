use log::Level;
use std::fs;
use std::path::PathBuf;
use std::sync::Once;

fn init_logger(min_level: Level, path: &PathBuf) {
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();

    let config = simplelog::ConfigBuilder::new()
        .add_filter_ignore_str("h2")
        .add_filter_ignore_str("hyper")
        .add_filter_ignore_str("mio")
        .add_filter_ignore_str("reqwest")
        .add_filter_ignore_str("rustls")
        .add_filter_ignore_str("rustyline")
        .add_filter_ignore_str("tokio_util")
        .add_filter_ignore_str("tonic")
        .add_filter_ignore_str("tower")
        .add_filter_ignore_str("tracing")
        .add_filter_ignore_str("ureq")
        .add_filter_ignore_str("want")
        .set_time_format_rfc3339()
        .build();

    simplelog::WriteLogger::init(min_level.to_level_filter(), config, log_file).unwrap();
}

static INIT_LOGGER_ONCE: Once = Once::new();

/// Call the function once before instantiating the library to get logs.
/// Subsequent calls will have no effect.
pub(crate) fn init_logger_once(min_level: Level, path: &PathBuf) {
    INIT_LOGGER_ONCE.call_once(|| init_logger(min_level, path));
}
