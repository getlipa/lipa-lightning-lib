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
    // Temporary migration error
    /// Migration of funds from legacy LDK wallet failed. Retry is recommended.
    FailedFundMigration,
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

    /// The payment failed. Likely the issue is on the receiver.
    /// There's no point in retrying this payment.
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

    /// The payment failed. Likely the issue is on the receiver.
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
}

impl Display for LnUrlPayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type LnUrlPayError = perro::Error<LnUrlPayErrorCode>;
pub type LnUrlPayResult<T> = std::result::Result<T, LnUrlPayError>;

#[derive(Debug, thiserror::Error)]
pub enum UnsupportedDataType {
    #[error("Bitcoin on-chain address")]
    BitcoinAddress,
    #[error("LNURL Auth")]
    LnUrlAuth,
    #[error("LNURL Withdraw")]
    LnUrlWithdraw,
    #[error("Lightning node id")]
    NodeId,
    #[error("URL")]
    Url,
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
