use eel::errors::RuntimeErrorCode as EelRuntimeErrorCode;
use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    // 3L runtime errors
    AuthServiceUnavailable,
    TopupServiceUnavailable,

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

impl RuntimeErrorCode {
    pub fn from_eel_runtime_error_code(eel_runtime_error_code: EelRuntimeErrorCode) -> Self {
        match eel_runtime_error_code {
            EelRuntimeErrorCode::EsploraServiceUnavailable => Self::EsploraServiceUnavailable,
            EelRuntimeErrorCode::LspServiceUnavailable => Self::LspServiceUnavailable,
            EelRuntimeErrorCode::RemoteStorageError => Self::RemoteStorageError,
            EelRuntimeErrorCode::NonExistingWallet => Self::NonExistingWallet,
        }
    }
}

pub type Error = perro::Error<RuntimeErrorCode>;
pub type Result<T> = std::result::Result<T, Error>;
