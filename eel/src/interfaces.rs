use crate::errors::Result;
use std::fmt::Debug;

pub trait RemoteStorage: Send + Sync + Debug {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> Result<Vec<String>>;

    fn object_exists(&self, bucket: String, key: String) -> Result<bool>;

    fn get_object(&self, bucket: String, key: String) -> Result<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> Result<()>;

    fn delete_object(&self, bucket: String, key: String) -> Result<()>;
}

pub trait Lsp: Send + Sync {
    fn channel_information(&self) -> Result<Vec<u8>>;

    /// Register a new incoming payment.
    ///
    /// # Return
    /// Returns non empty string with description in case of an error.
    fn register_payment(&self, encrypted_payment_info_blob: Vec<u8>) -> Result<()>;
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
