use crate::environment::EnvironmentCode;
use eel::config::TzConfig;

#[derive(Debug, Clone)]
pub struct Config {
    pub environment: EnvironmentCode,
    pub seed: Vec<u8>,
    pub fiat_currency: String,
    pub local_persistence_path: String,
    pub timezone_config: TzConfig,
    pub enable_file_logging: bool,
}
