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

pub trait LspCallback: Send + Sync {
    fn channel_information(&self) -> Result<Vec<u8>, LspError>;

    /// Register a new incoming payment.
    ///
    /// # Return
    /// Returns non empty string with description in case of an error.
    fn register_payment(&self, bytes: Vec<u8>) -> Result<(), LspError>;
}
