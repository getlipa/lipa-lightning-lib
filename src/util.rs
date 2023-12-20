use std::str::FromStr;
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
                .map(|byte| u8::from_str(byte.trim()))
                .collect::<Result<Vec<u8>, _>>();

            match hex_data {
                Ok(data) => format!("\"{}\"", encode(&data)),
                Err(_) => caps[0].to_string(), // if parsing fails, return original string
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_byte_arrays_by_hex_string() {
        let original = "Hello [15, 16, 255] world";
        let expected = "Hello \"0f10ff\" world";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, actual);
    }

    #[test]
    fn string_starts_and_ends_with_array_parsed_to_hex() {
        let original = "[186, 190] make some [192, 255, 238]";
        let expected = "\"babe\" make some \"c0ffee\"";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, actual);
    }

    #[test]
    fn arrays_within_words_parsed_to_hex() {
        let original = "Lipa W[161][30]t";
        let expected = "Lipa W\"a1\"\"1e\"t";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, actual);
    }

    #[test]
    fn empty_array_not_parsed_to_hex() {
        let original = "Hello [] world";
        let modified = replace_byte_arrays_by_hex_string(original);
        assert_eq!(original, modified);
    }

    #[test]
    fn flawed_byte_array_not_parsed_to_hex() {
        let original = "Hello [15, 16, 1234] world";
        let parsed = replace_byte_arrays_by_hex_string(original);
        assert_eq!(original, parsed);
    }
}
