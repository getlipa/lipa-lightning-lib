use std::fmt::{Display, Formatter};

/// A code that specifies the RuntimeError that occurred
#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    DecryptionFailed,
    InvoiceAmountNotInRange,
    VoucherHasExpired,
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;
pub type Result<T> = std::result::Result<T, Error>;
