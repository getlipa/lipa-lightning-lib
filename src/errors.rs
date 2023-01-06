//! LipaError enum with helper functions.
//!
//! # Examples
//!
//! ```ignore
//! fn foo(x: u32) -> LipaResult<String> {
//!     if x <= 10 {
//!         return Err(invalid_input("x must be greater than 10"));
//!     }
//!     foreign_function().map_to_runtime_error(RuntimeErrorCode::NotEnoughFunds, "Foreign code failed")?;
//!     internal_function().prefix_error("Internal function failed")?;
//!     another_internal_function().lift_invalid_input("Another failure")?;
//! }
//! ```

use std::fmt::{Display, Formatter};
use uniffi::ffi::foreigncallbacks::UnexpectedUniFFICallbackError;

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeErrorCode {
    EsploraServiceUnavailable,
    RgsServiceUnavailable,
    RgsUpdateError,
    LspServiceUnavailable,
    RemoteStorageServiceUnavailable,
    NoRouteFound,
    SendFailure,
    GenericError,
}

impl Display for RuntimeErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LipaError {
    /// Invalid input.
    /// Consider fixing the input and retrying the request.
    #[error("InvalidInput: {msg}")]
    InvalidInput { msg: String },

    /// Recoverable problem (e.g. network issue, problem with en external service).
    /// Consider retrying the request.
    #[error("RuntimeError: {code} - {msg}")]
    RuntimeError { code: RuntimeErrorCode, msg: String },

    /// Unrecoverable problem (e.g. internal invariant broken).
    /// Consider suggesting the user to report the issue to the developers.
    #[error("PermanentFailure: {msg}")]
    PermanentFailure { msg: String },
}

#[allow(dead_code)]
pub fn invalid_input<E: ToString>(e: E) -> LipaError {
    LipaError::InvalidInput { msg: e.to_string() }
}

#[allow(dead_code)]
pub fn runtime_error<E: ToString>(code: RuntimeErrorCode, e: E) -> LipaError {
    LipaError::RuntimeError {
        code,
        msg: e.to_string(),
    }
}

pub fn permanent_failure<E: ToString>(e: E) -> LipaError {
    LipaError::PermanentFailure { msg: e.to_string() }
}

pub type LipaResult<T> = Result<T, LipaError>;

pub trait LipaResultTrait<T> {
    /// Lift `InvalidInput` error into `PermanentFailure`.
    ///
    /// Use the method when you want to propagate an error from an internal
    /// function to the caller.
    /// Reasoning is that if you got `InvalidInput` it means you failed to
    /// validate the input for the internal function yourself, so for you it
    /// becomes `PermanentFailure`.
    fn lift_invalid_input(self) -> LipaResult<T>;

    fn prefix_error<M: ToString + 'static>(self, msg: M) -> LipaResult<T>;
}

impl<T> LipaResultTrait<T> for LipaResult<T> {
    fn lift_invalid_input(self) -> LipaResult<T> {
        self.map_err(|e| match e {
            LipaError::InvalidInput { msg } => LipaError::PermanentFailure {
                msg: format!("InvalidInput: {}", msg),
            },
            another_error => another_error,
        })
    }

    fn prefix_error<M: ToString + 'static>(self, prefix: M) -> LipaResult<T> {
        self.map_err(|e| match e {
            LipaError::InvalidInput { msg } => LipaError::InvalidInput {
                msg: format!("{}: {}", prefix.to_string(), msg),
            },
            LipaError::RuntimeError { code, msg } => LipaError::RuntimeError {
                code,
                msg: format!("{}: {}", prefix.to_string(), msg),
            },
            LipaError::PermanentFailure { msg } => LipaError::PermanentFailure {
                msg: format!("{}: {}", prefix.to_string(), msg),
            },
        })
    }
}

pub trait MapToLipaError<T, E: ToString> {
    fn map_to_invalid_input<M: ToString>(self, msg: M) -> LipaResult<T>;
    fn map_to_runtime_error<M: ToString>(self, code: RuntimeErrorCode, msg: M) -> LipaResult<T>;
    fn map_to_permanent_failure<M: ToString>(self, msg: M) -> LipaResult<T>;
}

impl<T, E: ToString> MapToLipaError<T, E> for Result<T, E> {
    fn map_to_invalid_input<M: ToString>(self, msg: M) -> LipaResult<T> {
        self.map_err(move |e| LipaError::InvalidInput {
            msg: format!("{}: {}", msg.to_string(), e.to_string()),
        })
    }

    fn map_to_runtime_error<M: ToString>(self, code: RuntimeErrorCode, msg: M) -> LipaResult<T> {
        self.map_err(move |e| LipaError::RuntimeError {
            code,
            msg: format!("{}: {}", msg.to_string(), e.to_string()),
        })
    }

    fn map_to_permanent_failure<M: ToString>(self, msg: M) -> LipaResult<T> {
        self.map_err(move |e| LipaError::PermanentFailure {
            msg: format!("{}: {}", msg.to_string(), e.to_string()),
        })
    }
}

pub trait MapToLipaErrorForUnitType<T> {
    fn map_to_invalid_input<M: ToString>(self, msg: M) -> LipaResult<T>;
    fn map_to_runtime_error<M: ToString>(self, code: RuntimeErrorCode, msg: M) -> LipaResult<T>;
    fn map_to_permanent_failure<M: ToString>(self, msg: M) -> LipaResult<T>;
}

impl<T> MapToLipaErrorForUnitType<T> for Result<T, ()> {
    fn map_to_invalid_input<M: ToString>(self, msg: M) -> LipaResult<T> {
        self.map_err(move |()| LipaError::InvalidInput {
            msg: msg.to_string(),
        })
    }

    fn map_to_runtime_error<M: ToString>(self, code: RuntimeErrorCode, msg: M) -> LipaResult<T> {
        self.map_err(move |()| LipaError::RuntimeError {
            code,
            msg: msg.to_string(),
        })
    }

    fn map_to_permanent_failure<M: ToString>(self, msg: M) -> LipaResult<T> {
        self.map_err(move |()| LipaError::PermanentFailure {
            msg: msg.to_string(),
        })
    }
}

pub trait OptionToError<T> {
    fn ok_or_invalid_input<M: ToString>(self, msg: M) -> LipaResult<T>;
    fn ok_or_runtime_error<M: ToString>(self, code: RuntimeErrorCode, msg: M) -> LipaResult<T>;
    fn ok_or_permanent_failure<M: ToString>(self, msg: M) -> LipaResult<T>;
}

impl<T> OptionToError<T> for Option<T> {
    fn ok_or_invalid_input<M: ToString>(self, msg: M) -> LipaResult<T> {
        self.ok_or_else(|| invalid_input(msg))
    }

    fn ok_or_runtime_error<M: ToString>(self, code: RuntimeErrorCode, msg: M) -> LipaResult<T> {
        self.ok_or_else(|| runtime_error(code, msg))
    }

    fn ok_or_permanent_failure<M: ToString>(self, msg: M) -> LipaResult<T> {
        self.ok_or_else(|| permanent_failure(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_to_lipa_errors() {
        use std::io::{Error, ErrorKind, Result};

        let io_error: Result<()> = Err(Error::new(ErrorKind::Other, "File not found"));
        let lipa_error = io_error
            .map_to_runtime_error(
                RuntimeErrorCode::RemoteStorageServiceUnavailable,
                "No backup",
            )
            .unwrap_err();
        assert_eq!(
            lipa_error.to_string(),
            "RuntimeError: RemoteStorageServiceUnavailable - No backup: File not found"
        );

        let error: std::result::Result<(), ()> = Err(());
        let lipa_error = error
            .map_to_runtime_error(
                RuntimeErrorCode::RemoteStorageServiceUnavailable,
                "No backup",
            )
            .unwrap_err();
        assert_eq!(
            lipa_error.to_string(),
            "RuntimeError: RemoteStorageServiceUnavailable - No backup"
        );
    }

    #[test]
    fn test_lift_invalid_input() {
        let result: LipaResult<()> =
            Err(invalid_input("Number must be positive")).lift_invalid_input();
        assert_eq!(
            result.unwrap_err().to_string(),
            "PermanentFailure: InvalidInput: Number must be positive"
        );

        let result: LipaResult<()> = Err(runtime_error(
            RuntimeErrorCode::EsploraServiceUnavailable,
            "Socket timeout",
        ))
        .lift_invalid_input();
        assert_eq!(
            result.unwrap_err().to_string(),
            "RuntimeError: EsploraServiceUnavailable - Socket timeout"
        );

        let result: LipaResult<()> =
            Err(permanent_failure("Devision by zero")).lift_invalid_input();
        assert_eq!(
            result.unwrap_err().to_string(),
            "PermanentFailure: Devision by zero"
        );
    }

    #[test]
    fn test_prefix_error() {
        let result: LipaResult<()> =
            Err(invalid_input("Number must be positive")).prefix_error("Invalid amount");
        assert_eq!(
            result.unwrap_err().to_string(),
            "InvalidInput: Invalid amount: Number must be positive"
        );
    }

    #[test]
    fn test_ok_or() {
        assert_eq!(Some(1).ok_or_permanent_failure("Value expected"), Ok(1));

        let none: Option<u32> = None;

        let error = none.ok_or_invalid_input("Value expected").unwrap_err();
        assert_eq!(error.to_string(), "InvalidInput: Value expected");

        let error = none
            .ok_or_runtime_error(
                RuntimeErrorCode::RemoteStorageServiceUnavailable,
                "Value expected",
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "RuntimeError: RemoteStorageServiceUnavailable - Value expected"
        );

        let error = none.ok_or_permanent_failure("Value expected").unwrap_err();
        assert_eq!(error.to_string(), "PermanentFailure: Value expected");
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    #[error("InvalidInput")]
    InvalidInput,

    #[error("RuntimeError")]
    RuntimeError,

    #[error("PermanentFailure")]
    PermanentFailure,

    #[error("UnexpectedUniFFICallbackError")]
    UnexpectedUniFFI,
}

impl From<UnexpectedUniFFICallbackError> for CallbackError {
    fn from(_error: UnexpectedUniFFICallbackError) -> Self {
        CallbackError::UnexpectedUniFFI
    }
}

pub type CallbackResult<T> = Result<T, CallbackError>;
