use crate::errors::Result;
use std::time::SystemTime;

pub trait RemoteStorage: Send + Sync {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> Result<Vec<String>>;

    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()>;

    fn delete_object(&self, bucket: String, key: String) -> Result<()>;
}

pub trait EventHandler: Send + Sync {
    fn payment_received(&self, payment_hash: String, amount_msat: u64);

    fn payment_sent(&self, payment_hash: String, payment_preimage: String, fee_paid_msat: u64);

    fn payment_failed(&self, payment_hash: String);

    fn channel_closed(&self, channel_id: String, reason: String);
}

#[derive(Clone)]
pub struct ExchangeRate {
    pub currency_code: String,
    pub rate: u32,
    pub updated_at: SystemTime,
}

pub trait ExchangeRateProvider: Send + Sync {
    fn query_all_exchange_rates(&self) -> Result<Vec<ExchangeRate>>;
}
