use crate::Network;
use breez_sdk_core::error::{LspErrorCode, PaymentErrorCode, SdkError, SdkResult};
use num_enum::TryFromPrimitive;
use perro::{invalid_input, permanent_failure, runtime_error};
use std::fmt::{Display, Formatter};

/// A code that specifies a service-related runtime error that occurred
#[derive(Debug, PartialEq, Eq)]
pub enum ServiceErrorCode {
    // 3L runtime errors
    /// The backend auth service is unavailable.
    AuthServiceUnavailable,
    OfferServiceUnavailable,
    /// The lsp service is unavailable. Could there be a loss of internet connection?
    LspServiceUnavailable,

    // Breez runtime errors
    /// Information about the remote node isn't cached and couldn't be accessed. Could be a network error.
    NodeUnavailable,
    // Temporary migration error
    /// Migration of funds from legacy LDK wallet failed. Retry is recommended.
    FailedFundMigration,
}

impl Display for ServiceErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<ErrorCode>;
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) trait MapTo3lError<T> {
    fn map_to_3l_error<M: ToString>(self, msg: M) -> Result<T>;
}

impl<T> MapTo3lError<T> for SdkResult<T> {
    fn map_to_3l_error<M: ToString>(self, msg: M) -> Result<T> {
        self.map_err(|e| match e {
            SdkError::LspError { code, err } => match code {
                LspErrorCode::ChannelOpeningNotAvailable => runtime_error(
                    ServiceErrorCode::LspServiceUnavailable.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                LspErrorCode::ConnectionFailed => runtime_error(
                    ServiceErrorCode::LspServiceUnavailable.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                LspErrorCode::LspNotFound => runtime_error(
                    ServiceErrorCode::LspServiceUnavailable.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
            },
            SdkError::OnchainError { err, .. } =>
            // TODO: map onchain errors. As soon as `sweep()` starts getting used, these errors will need to be mapped
            {
                permanent_failure(format!(
                    "Onchain error when no onchain funcionality was used: {}",
                    err
                ))
            }
            SdkError::PaymentError { code, err } => match code {
                PaymentErrorCode::AlreadyPaid => runtime_error(
                    PayErrorCode::AlreadyUsedInvoice.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                PaymentErrorCode::InvalidAmount => {
                    invalid_input(format!("{}: {}", msg.to_string(), err))
                }
                PaymentErrorCode::InvoiceExpired => runtime_error(
                    PayErrorCode::InvoiceExpired.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                PaymentErrorCode::PaymentFailed => runtime_error(
                    PayErrorCode::UnexpectedError.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                PaymentErrorCode::PaymentTimeout => runtime_error(
                    PayErrorCode::RetriesExhausted.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                PaymentErrorCode::RouteNotFound => runtime_error(
                    PayErrorCode::NoRouteFound.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
                PaymentErrorCode::RouteTooExpensive => runtime_error(
                    PayErrorCode::RouteTooExpensive.into(),
                    format!("{}: {}", msg.to_string(), err),
                ),
            },
            SdkError::ServiceError { err, .. } => runtime_error(
                ServiceErrorCode::NodeUnavailable.into(),
                format!("{}: {}", msg.to_string(), err),
            ),
            SdkError::ValueError { err, .. } => {
                invalid_input(format!("{}: {}", msg.to_string(), err))
            }
        })
    }
}

/// A code that specifies the payment-related runtime error that occurred.
#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PayErrorCode {
    /// The invoice has already expired.
    /// There's no point in retrying this payment
    InvoiceExpired,
    /// An already recognized invoice tried to be paid. Either a payment attempt is in progress or the invoice has already been paid.
    /// There's no point in retrying this payment
    AlreadyUsedInvoice,
    /// Not a single route was found.
    /// There's no point in retrying this payment
    NoRouteFound,
    /// The recipient has rejected the payment.
    /// It might make sense to retry the payment.
    RecipientRejected,
    /// Retry attempts or timeout was reached.
    /// It might make sense to retry the payment.
    RetriesExhausted,
    /// All possible routes failed.
    /// It might make sense to retry the payment.
    NoMoreRoutes,
    /// Indicates there is at least one available route but
    /// it wasn't tried due to the routing fees being above the configured limit
    ///
    /// *WARNING*: At the moment, this should be treated in the same way as
    /// [`PayErrorCode::NoMoreRoutes`] because there's no way for the user to
    /// change the max routing fees. Later, when that feature is available,
    /// showing a recommendation to increase the max fees is advised.
    RouteTooExpensive,
    /// The payment failed for an unknown reason
    UnexpectedError,
}

impl Display for PayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// A code that specifies the runtime error that occurred
#[derive(Debug, PartialEq)]
pub enum ErrorCode {
    Pay { code: PayErrorCode },
    Service { code: ServiceErrorCode },
}

impl From<PayErrorCode> for ErrorCode {
    fn from(code: PayErrorCode) -> ErrorCode {
        ErrorCode::Pay { code }
    }
}

impl From<ServiceErrorCode> for ErrorCode {
    fn from(code: ServiceErrorCode) -> ErrorCode {
        ErrorCode::Service { code }
    }
}

impl Display for ErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCode::Pay { code } => {
                write!(f, "PayError({code})")
            }
            ErrorCode::Service { code } => {
                write!(f, "ServiceError({code})")
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeInvoiceError {
    #[error("Parse error: {msg}")]
    ParseError { msg: String },
    #[error("Semantic error: {msg}")]
    SemanticError { msg: String },
    #[error("Network mismatch (expected {expected}, found {found})")]
    NetworkMismatch { expected: Network, found: Network },
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MnemonicError {
    /// Mnemonic has a word count that is not a multiple of 6.
    #[error("BadWordCount with count: {count}")]
    BadWordCount { count: u64 },
    /// Mnemonic contains an unknown word at the pointed index.
    #[error("UnknownWord at index: {index}")]
    UnknownWord { index: u64 },
    /// Entropy was not a multiple of 32 bits or between 128-256n bits in length.
    #[error("BadEntropyBitCount")]
    BadEntropyBitCount,
    /// The mnemonic has an invalid checksum.
    #[error("InvalidChecksum")]
    InvalidChecksum,
    /// The mnemonic can be interpreted as multiple languages.
    #[error("AmbiguousLanguages")]
    AmbiguousLanguages,
}

pub fn to_mnemonic_error(e: bip39::Error) -> MnemonicError {
    match e {
        bip39::Error::BadWordCount(count) => MnemonicError::BadWordCount {
            count: count as u64,
        },
        bip39::Error::UnknownWord(index) => MnemonicError::UnknownWord {
            index: index as u64,
        },
        bip39::Error::BadEntropyBitCount(_) => MnemonicError::BadEntropyBitCount,
        bip39::Error::InvalidChecksum => MnemonicError::InvalidChecksum,
        bip39::Error::AmbiguousLanguages(_) => MnemonicError::AmbiguousLanguages,
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SimpleError {
    #[error("SimpleError: {msg}")]
    Simple { msg: String },
}
