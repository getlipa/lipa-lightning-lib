use crate::{invalid_input, permanent_failure, runtime_error};

use breez_sdk_core::error::SendPaymentError;
use std::fmt::{Display, Formatter};

/// A code that specifies the RuntimeError that occurred
#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    // 3L runtime errors
    /// The backend auth service is unavailable.
    AuthServiceUnavailable,
    OfferServiceUnavailable,
    /// The lsp service is unavailable. Could there be a loss of internet connection?
    LspServiceUnavailable,
    /// The backup service is unavailable. Could there be a loss of internet connection?
    BackupServiceUnavailable,
    /// No backup was found for the provided mnemonic.
    BackupNotFound,

    // Breez runtime errors
    /// Information about the remote node isn't cached and couldn't be accessed. Could be a network error.
    NodeUnavailable,
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;
pub type Result<T> = std::result::Result<T, Error>;

/// A code that specifies the PayError that occurred.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum PayErrorCode {
    /// An already recognized invoice tried to be paid.
    /// Either a payment attempt is in progress or the invoice has already been paid.
    /// There's no point in retrying this payment.
    AlreadyUsedInvoice,

    /// The invoice has already expired.
    /// There's no point in retrying this payment.
    InvoiceExpired,

    /// Not a single route was found.
    /// There's no point in retrying this payment.
    NoRouteFound,

    /// A locally issued invoice tried to be paid. Self-payments are not supported.
    /// There's no point in retrying this payment.
    PayingToSelf,

    /// The payment failed for another reason. Might be an issue with the receiver.
    PaymentFailed,

    /// Payment timed out.
    /// It might make sense to retry the payment.
    PaymentTimeout,

    /// Route too expensive. The route's fee exceeds the settings.
    RouteTooExpensive,

    /// The remote lightning node is not available. Could be a network error.
    NodeUnavailable,

    /// An unexpected error occurred.
    /// This likely is a result of a bug within 3L/Breez SDK and should be reported to lipa.
    UnexpectedError,
}

impl Display for PayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type PayError = perro::Error<PayErrorCode>;
pub type PayResult<T> = std::result::Result<T, PayError>;

/// A code that specifies the LnUrlPayError that occurred.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum LnUrlPayErrorCode {
    /// LNURL server returned an error.
    LnUrlServerError,

    /// Not a single route was found.
    NoRouteFound,

    /// The payment failed for another reason. Might be an issue with the receiver.
    PaymentFailed,

    /// Payment timed out.
    /// It might make sense to retry the payment.
    PaymentTimeout,

    /// Route too expensive. The route's fee exceeds the settings.
    RouteTooExpensive,

    /// The remote lightning node or LNURL server is not available. Could be a network error.
    ServiceConnectivity,

    /// An unexpected error occurred.
    /// This likely is a result of a bug within 3L/Breez SDK and should be reported to lipa.
    UnexpectedError,

    /// The invoice is issued for another bitcoin network (e.g. testnet).
    InvalidNetwork,
}

impl Display for LnUrlPayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type LnUrlPayError = perro::Error<LnUrlPayErrorCode>;
pub type LnUrlPayResult<T> = std::result::Result<T, LnUrlPayError>;

/// A code that specifies the LnUrlWithdrawError that occurred.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum LnUrlWithdrawErrorCode {
    /// LNURL server returned an error.
    LnUrlServerError,

    /// The remote lightning node or LNURL server is not available. Could be a network error.
    ServiceConnectivity,

    /// An unexpected error occurred.
    /// This likely is a result of a bug within 3L/Breez SDK and should be reported to lipa.
    UnexpectedError,
}

impl Display for LnUrlWithdrawErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type LnUrlWithdrawError = perro::Error<LnUrlWithdrawErrorCode>;
pub type LnUrlWithdrawResult<T> = std::result::Result<T, LnUrlWithdrawError>;

#[derive(Debug, thiserror::Error)]
pub enum UnsupportedDataType {
    #[error("Bitcoin on-chain address")]
    BitcoinAddress,
    #[error("LNURL Auth")]
    LnUrlAuth,
    #[error("Lightning node id")]
    NodeId,
    #[error("URL")]
    Url,
    #[error("Network: {network}")]
    Network { network: String },
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeDataError {
    #[error("LNURL error: {msg}")]
    LnUrlError { msg: String },
    #[error("Unsupported data type: {typ}")]
    Unsupported { typ: UnsupportedDataType },
    #[error("Unrecognized data type: {msg}")]
    Unrecognized { msg: String },
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

pub(crate) fn map_send_payment_error(err: SendPaymentError) -> PayError {
    match err {
        SendPaymentError::AlreadyPaid => {
            runtime_error(PayErrorCode::AlreadyUsedInvoice, String::new())
        }
        SendPaymentError::Generic { err } => runtime_error(PayErrorCode::UnexpectedError, err),
        SendPaymentError::InvalidAmount { err } => invalid_input(format!("Invalid amount: {err}")),
        SendPaymentError::InvalidInvoice { err } => {
            invalid_input(format!("Invalid invoice: {err}"))
        }
        SendPaymentError::InvoiceExpired { err } => {
            runtime_error(PayErrorCode::InvoiceExpired, err)
        }
        SendPaymentError::PaymentFailed { err } => runtime_error(PayErrorCode::PaymentFailed, err),
        SendPaymentError::PaymentTimeout { err } => {
            runtime_error(PayErrorCode::PaymentTimeout, err)
        }
        SendPaymentError::RouteNotFound { err } => runtime_error(PayErrorCode::NoRouteFound, err),
        SendPaymentError::RouteTooExpensive { err } => {
            runtime_error(PayErrorCode::RouteTooExpensive, err)
        }
        SendPaymentError::ServiceConnectivity { err } => {
            runtime_error(PayErrorCode::NodeUnavailable, err)
        }
        SendPaymentError::InvalidNetwork { err } => {
            invalid_input(format!("Invalid network: {err}"))
        }
    }
}

pub(crate) fn map_lnurl_pay_error(error: breez_sdk_core::LnUrlPayError) -> LnUrlPayError {
    use breez_sdk_core::LnUrlPayError;
    match error {
        LnUrlPayError::InvalidUri { err } => invalid_input(format!("InvalidUri: {err}")),
        LnUrlPayError::AlreadyPaid => permanent_failure("LNURL pay invoice has been already paid"),
        LnUrlPayError::Generic { err } => runtime_error(LnUrlPayErrorCode::UnexpectedError, err),
        LnUrlPayError::InvalidAmount { err } => runtime_error(
            LnUrlPayErrorCode::LnUrlServerError,
            format!("Invalid amount in the invoice from LNURL pay server: {err}"),
        ),
        LnUrlPayError::InvalidInvoice { err } => runtime_error(
            LnUrlPayErrorCode::LnUrlServerError,
            format!("Invalid invoice from LNURL pay server: {err}"),
        ),
        LnUrlPayError::InvoiceExpired { err } => {
            permanent_failure(format!("Invoice for LNURL pay has already expired: {err}"))
        }
        LnUrlPayError::PaymentFailed { err } => {
            runtime_error(LnUrlPayErrorCode::PaymentFailed, err)
        }
        LnUrlPayError::PaymentTimeout { err } => {
            runtime_error(LnUrlPayErrorCode::PaymentTimeout, err)
        }
        LnUrlPayError::RouteNotFound { err } => runtime_error(LnUrlPayErrorCode::NoRouteFound, err),
        LnUrlPayError::RouteTooExpensive { err } => {
            runtime_error(LnUrlPayErrorCode::RouteTooExpensive, err)
        }
        LnUrlPayError::ServiceConnectivity { err } => {
            runtime_error(LnUrlPayErrorCode::ServiceConnectivity, err)
        }
        LnUrlPayError::InvalidNetwork { err } => {
            runtime_error(LnUrlPayErrorCode::InvalidNetwork, err)
        }
    }
}

pub(crate) fn map_lnurl_withdraw_error(
    error: breez_sdk_core::LnUrlWithdrawError,
) -> LnUrlWithdrawError {
    use breez_sdk_core::LnUrlWithdrawError;
    match error {
        LnUrlWithdrawError::Generic { err } => {
            runtime_error(LnUrlWithdrawErrorCode::UnexpectedError, err)
        }
        LnUrlWithdrawError::InvalidAmount { err } => {
            invalid_input(format!("Invalid withdraw amount: {err}"))
        }
        LnUrlWithdrawError::InvalidInvoice { err } => {
            permanent_failure(format!("Invalid invoice was created locally: {err}"))
        }
        LnUrlWithdrawError::InvalidUri { err } => invalid_input(format!("InvalidUri: {err}")),
        LnUrlWithdrawError::ServiceConnectivity { err } => {
            runtime_error(LnUrlWithdrawErrorCode::ServiceConnectivity, err)
        }
        LnUrlWithdrawError::InvoiceNoRoutingHints { err } => permanent_failure(format!(
            "A locally created invoice doesn't have any routing hints: {err}"
        )),
    }
}

/// A code that specifies the NotificationHandlingError that occurred.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum NotificationHandlingErrorCode {
    /// Information about the remote node isn't cached and couldn't be accessed.
    /// Could be a network error.
    NodeUnavailable,
    /// The notification payload implied the existence of an in-progress swap, but it couldn't be
    /// found. Maybe another instance of the wallet completed the swap.
    InProgressSwapNotFound,
    /// The notification payload implied the existence of an incoming payment, but it was not
    /// received in time. Starting the app might help complete the payment.
    ExpectedPaymentNotReceived,
    /// An inbound payment was rejected as it required opening a new channel.
    InsufficientInboundLiquidity,
    /// A request to one of lipa's services failed.
    LipaServiceUnavailable,
    /// The notification payload is disabled in the provided
    /// [`NotificationToggles`](crate::notification_handling::NotificationToggles).
    NotificationDisabledInNotificationToggles,
}

impl Display for NotificationHandlingErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type NotificationHandlingError = perro::Error<NotificationHandlingErrorCode>;
pub type NotificationHandlingResult<T> = std::result::Result<T, NotificationHandlingError>;

impl NotificationHandlingErrorCode {
    pub(crate) fn from_runtime_error(_error: RuntimeErrorCode) -> Self {
        Self::NodeUnavailable
    }
}

/// Enum representing possible errors why parsing could fail.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Parsing failed because parsed string was not complete.
    /// Additional characters are needed to make the string valid.
    /// It makes parsed string a valid prefix of a valid string.
    #[error("Incomplete")]
    Incomplete,

    /// Parsing failed because an unexpected character at position `at` was met.
    /// The character **has to be removed**.
    #[error("InvalidCharacter at {at}")]
    InvalidCharacter { at: u32 },
}

impl From<parser::ParseError> for ParseError {
    fn from(error: parser::ParseError) -> Self {
        match error {
            parser::ParseError::Incomplete => ParseError::Incomplete,
            parser::ParseError::UnexpectedCharacter(at) | parser::ParseError::ExcessSuffix(at) => {
                ParseError::InvalidCharacter { at: at as u32 }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParsePhoneNumberPrefixError {
    #[error("Incomplete")]
    Incomplete,
    #[error("InvalidCharacter at {at}")]
    InvalidCharacter { at: u32 },
    #[error("UnsupportedCountry")]
    UnsupportedCountry,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParsePhoneNumberError {
    #[error("ParsingError")]
    ParsingError,
    #[error("MissingCountryCode")]
    MissingCountryCode,
    #[error("InvalidCountryCode")]
    InvalidCountryCode,
    #[error("InvalidPhoneNumber")]
    InvalidPhoneNumber,
    #[error("UnsupportedCountry")]
    UnsupportedCountry,
}
