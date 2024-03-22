use crate::environment::EnvironmentCode;
use std::time::SystemTime;

/// An object that holds all configuration needed to start a LightningNode instance.
#[derive(Debug, Clone)]
pub struct Config {
    /// A code of the environment to run the node.
    pub environment: EnvironmentCode,
    /// The seed derived from the mnemonic optionally including a pass phrase
    pub seed: Vec<u8>,
    /// ISO 4217 currency code. The backend does not support all of them, but supports at least USD
    /// and EUR, so it is safe to default to one of them. Providing an invalid code will result in
    /// missing fiat values for payments.
    pub fiat_currency: String,
    /// A path on the local filesystem where this library will directly persist data. Only the
    /// current instance of the app should have access to the provided directory. On app
    /// uninstall/deletion, the directory should be purged.
    pub local_persistence_path: String,
    /// A timezone configuration object.
    pub timezone_config: TzConfig,
    /// If a value is provided, logs using the provided level will be created in the provided
    /// `local_persistence_path`.
    pub file_logging_level: Option<log::Level>,
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

impl TzTime {
    pub(crate) fn new(time: SystemTime, tz_config: TzConfig) -> Self {
        Self {
            time,
            timezone_id: tz_config.timezone_id,
            timezone_utc_offset_secs: tz_config.timezone_utc_offset_secs,
        }
    }
}
