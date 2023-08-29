use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    // 3L runtime errors
    AuthServiceUnavailable,
    OfferServiceUnavailable,

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
