use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    AuthServiceUnvailable,
    EsploraServiceUnavailable,
    ExchangeRateProviderUnavailable,
    LspServiceUnavailable,
    RemoteStorageError,
    RgsServiceUnavailable,
    RgsUpdateError,

    ObjectNotFound,

    NonExistingWallet,

    AlreadyUsedInvoice,
    InvoiceExpired,
    PayingToSelf,
    NoRouteFound,

    GenericError,
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;

pub type Result<T> = std::result::Result<T, perro::Error<RuntimeErrorCode>>;
