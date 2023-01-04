use crate::errors::CallbackResult;
use std::fmt::Debug;

pub trait RemoteStorageCallback: Send + Sync + Debug {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> CallbackResult<Vec<String>>;

    fn object_exists(&self, bucket: String, key: String) -> CallbackResult<bool>;

    fn get_object(&self, bucket: String, key: String) -> CallbackResult<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> CallbackResult<()>;

    fn delete_object(&self, bucket: String, key: String) -> CallbackResult<()>;
}

pub trait LspCallback: Send + Sync {
    fn channel_information(&self) -> CallbackResult<Vec<u8>>;

    /// Register a new incoming payment.
    ///
    /// # Return
    /// Returns non empty string with description in case of an error.
    fn register_payment(&self, encrypted_payment_info_blob: Vec<u8>) -> CallbackResult<()>;
}

pub trait EventsCallback: Send + Sync {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) -> CallbackResult<()>;

    fn channel_closed(&self, channel_id: String, reason: String) -> CallbackResult<()>;

    fn payment_sent(
        &self,
        payment_hash: String,
        payment_preimage: String,
        fee_paid_msat: u64,
    ) -> CallbackResult<()>;

    fn payment_failed(&self, payment_hash: String) -> CallbackResult<()>;
}
