use std::time::SystemTime;

/// An object that holds all configuration needed to start a LightningNode instance.
#[derive(Debug, Clone)]
pub struct Config {
    /// The seed derived from the mnemonic optionally including a pass phrase
    pub seed: Vec<u8>,
    /// ISO 4217 currency code. The backend does not support all of them, but supports at least USD
    /// and EUR, so it is safe to default to one of them. Providing an invalid code will result in
    /// missing fiat values for payments.
    ///
    /// The provided value is used as a default. After the first time the node is started,
    /// this config starts being ignored. Changing the fiat currency can be done using
    /// [`crate::LightningNode::change_fiat_currency`].
    pub default_fiat_currency: String,
    /// A path on the local filesystem where this library will directly persist data. Only the
    /// current instance of the app should have access to the provided directory. On app
    /// uninstall/deletion, the directory should be purged.
    pub local_persistence_path: String,
    /// A timezone configuration object.
    pub timezone_config: TzConfig,
    /// If a value is provided, logs using the provided level will be created in the provided
    /// `local_persistence_path`.
    pub file_logging_level: Option<log::Level>,
    /// The list of allowed countries for the use of phone numbers as identifiers.
    pub phone_number_allowed_countries_iso_3166_1_alpha_2: Vec<String>,
    pub remote_services_config: RemoteServicesConfig,
    pub breez_sdk_config: BreezSdkConfig,
    pub max_routing_fee_config: MaxRoutingFeeConfig,
    pub receive_limits_config: ReceiveLimitsConfig,
}

#[derive(Debug, Clone)]
pub struct RemoteServicesConfig {
    /// lipa's backend URL.
    pub backend_url: String,
    /// Pocket's backend URL.
    pub pocket_url: String,
    /// Base URL used to construct the webhook URL used for notifications.
    pub notification_webhook_base_url: String,
    /// Secret used to encrypt the wallet's ID before being added to the webhook URL.
    pub notification_webhook_secret_hex: String,
    /// The domain used in lipa Lightning Addresses.
    pub lipa_lightning_domain: String,
}

#[derive(Debug, Clone)]
pub struct MaxRoutingFeeConfig {
    /// Routing fees will be limited to relative per myriad provided here.
    pub max_routing_fee_permyriad: u16,
    /// When the fee is lower or equal to this value, the relative limit is ignored.
    pub max_routing_fee_exempt_fee_sats: u64,
}

#[derive(Debug, Clone)]
pub struct BreezSdkConfig {
    pub breez_sdk_api_key: String,
    pub breez_sdk_partner_certificate: String,
    pub breez_sdk_partner_key: String,
}

#[derive(Debug, Clone)]
pub struct ReceiveLimitsConfig {
    pub max_receive_amount_sat: u64,
    pub min_receive_channel_open_fee_multiplier: f64,
}

/// An object that holds timezone configuration values necessary for 3L to do timestamp annotation. These values get tied
/// together with every timestamp persisted in the local payment database.
#[derive(Clone, Debug, Default, PartialEq)]
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

pub(crate) trait WithTimezone {
    fn with_timezone(self, tz_config: TzConfig) -> TzTime;
}

impl WithTimezone for SystemTime {
    fn with_timezone(self, tz_config: TzConfig) -> TzTime {
        TzTime {
            time: self,
            timezone_id: tz_config.timezone_id,
            timezone_utc_offset_secs: tz_config.timezone_utc_offset_secs,
        }
    }
}
