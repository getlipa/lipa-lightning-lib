use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    AuthServiceUnvailable,
    EsploraServiceUnavailable,
    LspServiceUnavailable,
    RemoteStorageError,
    NonExistingWallet,
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq)]
pub enum PayErrorCode {
    InvoiceExpired,
    AlreadyUsedInvoice,
    PayingToSelf,
    NoRouteFound,
    RecipientRejected,
    RetriesExhausted,
    NoMoreRoutes,
}

impl Display for PayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type PayError = perro::Error<PayErrorCode>;
pub type PayResult<T> = std::result::Result<T, PayError>;

#[derive(Debug, PartialEq, Eq)]
pub enum InternalRuntimeErrorCode {
    ExchangeRateProviderUnavailable,
    RgsUpdateError,
    RgsServiceUnavailable,
}

impl Display for InternalRuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

type InternalError = perro::Error<InternalRuntimeErrorCode>;
pub type InternalResult<T> = std::result::Result<T, InternalError>;
