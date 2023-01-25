use crate::errors::LipaResult;
use std::fmt::Debug;

pub trait RemoteStorageCallback: Send + Sync + Debug {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> LipaResult<Vec<String>>;

    fn object_exists(&self, bucket: String, key: String) -> LipaResult<bool>;

    fn get_object(&self, bucket: String, key: String) -> LipaResult<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> LipaResult<()>;

    fn delete_object(&self, bucket: String, key: String) -> LipaResult<()>;
}

pub trait LspCallback: Send + Sync {
    fn channel_information(&self) -> LipaResult<Vec<u8>>;

    /// Register a new incoming payment.
    ///
    /// # Return
    /// Returns non empty string with description in case of an error.
    fn register_payment(&self, encrypted_payment_info_blob: Vec<u8>) -> LipaResult<()>;
}

pub trait EventsCallback: Send + Sync {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) -> LipaResult<()>;

    fn channel_closed(&self, channel_id: String, reason: String) -> LipaResult<()>;

    fn payment_sent(
        &self,
        payment_hash: String,
        payment_preimage: String,
        fee_paid_msat: u64,
    ) -> LipaResult<()>;

    fn payment_failed(&self, payment_hash: String) -> LipaResult<()>;
}
