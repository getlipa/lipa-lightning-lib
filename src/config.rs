use crate::environment::EnvironmentCode;
use std::time::SystemTime;

/// An object that holds all configuration needed to start a LightningNode instance.
///
/// Fields:
/// * environment - a code of the environment to run the node.
/// * seed - the seed derived from the mnemonic optionally including a pass phrase
/// * fiat_currency - ISO 4217 currency code. The backend does not support all of them,
///     but supports at least USD and EUR, so it is safe to default to one of them.
///     Providing an invalid code will result in missing fiat values for payments.
/// * local_persistence_path - a path on the local filesystem where this library will directly persist data.
///      Only the current instance of the app should have access to the provided directory.
///      On app uninstall/deletion, the directory should be purged.
/// * timezone_config - a timezone configuration object
/// * enable_file_logging - if set to true, logs will be created in the provided local_persistence_path
#[derive(Debug, Clone)]
pub struct Config {
    pub environment: EnvironmentCode,
    pub seed: Vec<u8>,
    pub fiat_currency: String,
    pub local_persistence_path: String,
    pub timezone_config: TzConfig,
    pub enable_file_logging: bool,
}

/// An object that holds timezone configuration values necessary for 3L to do timestamp annotation. These values get tied
/// together with every timestamp persisted in the local payment database.
#[derive(Clone, Debug, PartialEq)]
pub struct TzConfig {
    /// String identifier whose format is completely arbitrary and can be chosen by the user
    pub timezone_id: String,
    /// Offset from the UTC timezone in seconds
    pub timezone_utc_offset_secs: i32,
}

/// A UTC timestamp accompanied by the ID of the timezone on which it was recorded and the respective UTC offset.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct TzTime {
    pub time: SystemTime,
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}
