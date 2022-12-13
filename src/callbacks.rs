use crate::errors::{CallbackResult, LspError};
use std::fmt::Debug;

pub trait RemoteStorageCallback: Send + Sync + Debug {
    fn check_health(&self) -> bool;

    fn list_objects(&self, bucket: String) -> CallbackResult<Vec<String>>;

    fn object_exists(&self, bucket: String, key: String) -> CallbackResult<bool>;

    fn get_object(&self, bucket: String, key: String) -> CallbackResult<Vec<u8>>;

    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> CallbackResult<()>;

    fn delete_object(&self, bucket: String, key: String) -> CallbackResult<()>;
}

pub trait RedundantStorageCallback: Send + Sync + Debug {
    fn object_exists(&self, bucket: String, key: String) -> bool;

    fn get_object(&self, bucket: String, key: String) -> Vec<u8>;

    /// Check health of the local and remote storage.
    /// The library will likely call this method before starting a transaction.
    /// Hint: request and cache an access tocken if needed.
    ///
    /// Returning `false` for `monitors` bucket will likely result in the
    /// library rejecting to start a transaction.
    fn check_health(&self, bucket: String) -> bool;

    /// Atomically put an object in the bucket (create the bucket if it does not exists).
    ///
    /// # Return
    /// Returns `true` if successful and `false` otherwise.
    ///
    /// Must only return after being certain that data was persisted safely.
    /// Failure to do so for `monitors` bucket may result in loss of funds.
    ///
    /// Returning `false` for `monitors` bucket will likely result in a channel
    /// being force-closed.
    fn put_object(&self, bucket: String, key: String, value: Vec<u8>) -> bool;

    /// List objects in the given bucket.
    ///
    /// # Return
    /// Returns a list of object keys present in the bucket.
    fn list_objects(&self, bucket: String) -> Vec<String>;

    fn delete_object(&self, bucket: String, key: String) -> bool;
}

pub trait LspCallback: Send + Sync {
    fn channel_information(&self) -> Result<Vec<u8>, LspError>;

    /// Register a new incoming payment.
    ///
    /// # Return
    /// Returns non empty string with description in case of an error.
    fn register_payment(&self, bytes: Vec<u8>) -> Result<(), LspError>;
}
