use crate::RuntimeErrorCode;
use hex::encode;
use log::{error, log, Level};
use perro::{MapToError, OptionToError};
use regex::{Captures, Regex};
use std::str;
use std::time::{Duration, SystemTime};

pub(crate) fn unix_timestamp_to_system_time(timestamp: u64) -> SystemTime {
    let duration = Duration::from_secs(timestamp);
    SystemTime::UNIX_EPOCH + duration
}

// Replaces all occurrences of byte arrays with their hex representation:
// 'Hello [15, 16, 255] world' -> 'Hello "0f10ff" world'
pub(crate) fn replace_byte_arrays_by_hex_string(original: &str) -> String {
    try_replacing_byte_arrays_by_hex_string(original).unwrap_or_else(|e| {
        error!("Failed to replace byte arrays by hex string: {}", e);
        original.to_string()
    })
}
fn try_replacing_byte_arrays_by_hex_string(
    original: &str,
) -> Result<String, perro::Error<RuntimeErrorCode>> {
    let byte_array_pattern = Regex::new(r"\[([\d\s,]+)]")
        .map_to_permanent_failure("Invalid regex to replace byte arrays")?;

    replace_all(&byte_array_pattern, original, |caps: &Captures| {
        let bytes_as_string = caps
            .get(1)
            .ok_or_permanent_failure("Captures::get(1) returned None")?
            .as_str();
        let bytes = bytes_as_string
            .split(',')
            .map(|byte| byte.trim().parse::<u8>())
            .collect::<Result<Vec<u8>, _>>()
            .map_to_permanent_failure(format!(
                "Failed to parse into byte array: {}",
                bytes_as_string
            ))?;

        Ok(encode(bytes))
    })
}

fn replace_all(
    re: &Regex,
    original: &str,
    replacement: impl Fn(&Captures) -> Result<String, perro::Error<RuntimeErrorCode>>,
) -> Result<String, perro::Error<RuntimeErrorCode>> {
    let mut new = String::new();
    let mut last_match = 0;
    for caps in re.captures_iter(original) {
        let m = caps
            .get(0)
            .ok_or_permanent_failure("Captures::get(0) returned None")?;
        new.push_str(&original[last_match..m.start()]);
        new.push('\"');
        new.push_str(&replacement(&caps)?);
        new.push('\"');
        last_match = m.end();
    }
    new.push_str(&original[last_match..]);
    Ok(new)
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
        assert_eq!(expected, &actual);
    }

    #[test]
    fn string_starts_and_ends_with_array_parsed_to_hex() {
        let original = "[186, 190] make some [192, 255, 238]";
        let expected = "\"babe\" make some \"c0ffee\"";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, &actual);
    }

    #[test]
    fn arrays_within_words_parsed_to_hex() {
        let original = "Lipa W[161][30]t";
        let expected = "Lipa W\"a1\"\"1e\"t";
        let actual = replace_byte_arrays_by_hex_string(original);
        assert_eq!(expected, &actual);
    }

    #[test]
    fn empty_array_not_parsed_to_hex() {
        let original = "Hello [] world";
        let modified = replace_byte_arrays_by_hex_string(original);
        assert_eq!(original, &modified);
    }

    #[test]
    fn flawed_byte_array_not_parsed_to_hex() {
        let original = "Hello [15, 16, 1234] world";
        let parsed = replace_byte_arrays_by_hex_string(original);
        assert_eq!(original, &parsed);
    }
}
