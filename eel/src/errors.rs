use lightning::events::PaymentFailureReason;
use num_enum::TryFromPrimitive;
use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
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

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PayErrorCode {
    InvoiceExpired,
    AlreadyUsedInvoice,
    PayingToSelf,
    NoRouteFound,
    RecipientRejected,
    RetriesExhausted,
    NoMoreRoutes,
    UnexpectedError,
}

impl PayErrorCode {
    pub(crate) fn from_failure_reason(reason: PaymentFailureReason) -> Self {
        match reason {
            PaymentFailureReason::RecipientRejected => Self::RecipientRejected,
            PaymentFailureReason::UserAbandoned => Self::UnexpectedError,
            PaymentFailureReason::RetriesExhausted => Self::RetriesExhausted,
            PaymentFailureReason::PaymentExpired => Self::InvoiceExpired,
            PaymentFailureReason::RouteNotFound => Self::NoMoreRoutes,
            PaymentFailureReason::UnexpectedError => Self::UnexpectedError,
        }
    }
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
