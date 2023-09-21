use crate::environment::EnvironmentCode;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Config {
    pub environment: EnvironmentCode,
    pub seed: Vec<u8>,
    pub fiat_currency: String,
    pub local_persistence_path: String,
    pub timezone_config: TzConfig,
    pub enable_file_logging: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TzConfig {
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct TzTime {
    pub time: SystemTime,
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}
