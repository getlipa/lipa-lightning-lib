use log::{log, Level};
use std::time::{Duration, SystemTime};

pub(crate) fn unix_timestamp_to_system_time(timestamp: u64) -> SystemTime {
    let duration = Duration::from_secs(timestamp);
    SystemTime::UNIX_EPOCH + duration
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
