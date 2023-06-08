use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    AuthServiceUnvailable,
    EsploraServiceUnavailable,
    ExchangeRateProviderUnavailable,
    LspServiceUnavailable,
    RemoteStorageError,

    NonExistingWallet,

    GenericError,
    ObjectNotFound,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PayErrorCode {
    AlreadyUsedInvoice,
    InvoiceNetworkMismatch,
    InvoiceExpired,
    PayingToSelf,
    NoRouteFound,
    SendFailure,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InternalErrorCode {
    RgsServiceUnavailable, // The rapid gossip sync service is unavailable. Could there be a loss of internet connection?
    RgsUpdateError,        // Failed to apply update. Maybe retry?
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Display for PayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Display for InternalErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;
pub type PayError = perro::Error<PayErrorCode>;
pub type InternalError = perro::Error<InternalErrorCode>;

pub type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;
pub type PayResult<T> = std::result::Result<T, perro::Error<PayErrorCode>>;
pub type InternalResult<T> = std::result::Result<T, perro::Error<InternalErrorCode>>;
