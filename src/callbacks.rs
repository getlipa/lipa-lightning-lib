use std::fmt::Debug;
use uniffi::UnexpectedUniFFICallbackError;

#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    #[error("InvalidInput")]
    InvalidInput,

    #[error("RuntimeError")]
    RuntimeError,

    #[error("PermanentFailure")]
    PermanentFailure,

    #[error("UnexpectedUniFFICallbackError")]
    UnexpectedUniFFI,
}

impl From<UnexpectedUniFFICallbackError> for CallbackError {
    fn from(_error: UnexpectedUniFFICallbackError) -> Self {
        CallbackError::UnexpectedUniFFI
    }
}

pub type CallbackResult<T> = Result<T, CallbackError>;

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
