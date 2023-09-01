use crate::Network;
use num_enum::TryFromPrimitive;
use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    // 3L runtime errors
    AuthServiceUnavailable,
    OfferServiceUnavailable,
    ExchangeRateProviderUnavailable,

    // Eel runtime errors
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

impl Display for PayErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub type PayError = perro::Error<PayErrorCode>;
pub type PayResult<T> = std::result::Result<T, PayError>;

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
    #[error("BadWordCount with count: {count}")]
    BadWordCount { count: u64 },
    #[error("UnknownWord at index: {index}")]
    UnknownWord { index: u64 },
    #[error("BadEntropyBitCount")]
    BadEntropyBitCount,
    #[error("InvalidChecksum")]
    InvalidChecksum,
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
