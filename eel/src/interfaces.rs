use crate::errors::Result;

pub trait RemoteStorage: Send + Sync {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> Result<Vec<String>>;

    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()>;

    fn delete_object(&self, bucket: String, key: String) -> Result<()>;
}

pub trait EventHandler: Send + Sync {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) -> Result<()>;

    fn channel_closed(&self, channel_id: String, reason: String) -> Result<()>;

    fn payment_sent(
        &self,
        payment_hash: String,
        payment_preimage: String,
        fee_paid_msat: u64,
    ) -> Result<()>;

    fn payment_failed(&self, payment_hash: String) -> Result<()>;
}

#[derive(Clone)]
pub struct ExchangeRates {
    pub default_currency: u32,
    pub usd: u32,
}

pub trait ExchangeRateProvider: Send + Sync {
    fn query_exchange_rate(&self, code: String) -> Result<u32>;
}
