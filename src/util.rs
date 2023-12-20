use hex::encode;
use log::{log, Level};
use regex::Regex;
use std::time::{Duration, SystemTime};

pub(crate) fn unix_timestamp_to_system_time(timestamp: u64) -> SystemTime {
    let duration = Duration::from_secs(timestamp);
    SystemTime::UNIX_EPOCH + duration
}

// Replaces all occurrences of byte arrays with their hex representation:
// 'Hello [15, 16, 255] world' -> 'Hello "0f10ff" world'
pub(crate) fn replace_byte_arrays_by_hex_string(original: &str) -> String {
    let byte_array_pattern = Regex::new(r"\[([\d\s,]+)\]").unwrap();

    byte_array_pattern
        .replace_all(original, |caps: &regex::Captures| {
            let byte_array = caps.get(1).unwrap().as_str();
            let hex_data = byte_array
                .split(',')
                .map(|byte| u8::from_str_radix(byte.trim(), 10).unwrap())
                .collect::<Vec<u8>>();
            format!("\"{}\"", encode(&hex_data))
        })
        .to_string()
}

pub(crate) trait LogIgnoreError {
    fn log_ignore_error(self, level: Level, message: &str);
}

impl<T, E: std::fmt::Display> LogIgnoreError for Result<T, E> {
    fn log_ignore_error(self, level: Level, message: &str) {
        if let Err(e) = self {
            log!(level, "{message}: {e}")
        }
    }
}
