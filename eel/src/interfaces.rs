use std::fmt::{Display, Formatter};
use std::time::SystemTime;

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    Error,
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;
pub type Result<T> = std::result::Result<T, Error>;

pub trait RemoteStorage: Send + Sync {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> crate::errors::Result<Vec<String>>;

    fn get_object(&self, bucket: String, key: String) -> crate::errors::Result<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> crate::errors::Result<()>;

    fn delete_object(&self, bucket: String, key: String) -> crate::errors::Result<()>;
}

pub trait EventHandler: Send + Sync {
    fn payment_received(&self, payment_hash: String);

    fn payment_sent(&self, payment_hash: String, payment_preimage: String);

    fn payment_failed(&self, payment_hash: String);

    fn channel_closed(&self, channel_id: String, reason: String);
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ExchangeRate {
    pub currency_code: String,
    pub rate: u32,
    pub updated_at: SystemTime,
}

pub trait ExchangeRateProvider: Send + Sync {
    fn query_all_exchange_rates(&self) -> Result<Vec<ExchangeRate>>;
}
