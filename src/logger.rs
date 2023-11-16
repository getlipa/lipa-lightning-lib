use crate::errors::Result;

use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use log::Level;
use perro::MapToError;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

fn init_logger(min_level: Level, path: &Path) {
    let base_log_file = path.join("logs.txt");

    // Main log file ~25MB + 7 compressed older logs <2MB = Total storage < 40MB
    // Logs provide latest 800K logged lines
    let rotated_log = FileRotate::new(
        base_log_file,
        AppendCount::new(7),
        ContentLimit::Lines(100_000),
        Compression::OnRotate(0),
        None,
    );

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

    simplelog::WriteLogger::init(min_level.to_level_filter(), config, rotated_log).unwrap();
}

static INIT_LOGGER_ONCE: Once = Once::new();

/// Call the function once before instantiating the library to get logs.
/// Subsequent calls will have no effect.
pub(crate) fn init_logger_once(min_level: Level, path: &PathBuf) -> Result<()> {
    fs::create_dir_all(path)
        .map_to_permanent_failure(format!("Failed to create directory: {path:?}"))?;
    INIT_LOGGER_ONCE.call_once(|| init_logger(min_level, path));
    Ok(())
}
