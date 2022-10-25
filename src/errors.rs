use uniffi::ffi::foreigncallbacks::UnexpectedUniFFICallbackError;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Invalid input.
    /// Consider fixing the input and retrying the request.
    #[error("InvalidInput: {message}")]
    InvalidInput { message: String },
    /// Recoverable problem (e.g. network issue, problem with en external service).
    /// Consider retrying the request.
    #[error("RuntimeError: {message}")]
    RuntimeError { message: String },
    /// Unrecoverable problem (e.g. internal invariant broken).
    /// Consider reporting.
    #[error("PermanentFailure: {message}")]
    PermanentFailure { message: String },
}

pub fn invalid_input<T: ToString>(e: T) -> Error {
    Error::InvalidInput {
        message: e.to_string(),
    }
}

pub fn invalid_input_with<T: ToString>(message: String) -> Box<dyn FnOnce(T) -> Error> {
    Box::new(move |e: T| Error::InvalidInput {
        message: format!("{}: {}", message, e.to_string()),
    })
}

pub fn runtime_error<T: ToString>(e: T) -> Error {
    Error::RuntimeError {
        message: e.to_string(),
    }
}

pub fn runtime_error_with<T: ToString>(message: String) -> Box<dyn FnOnce(T) -> Error> {
    Box::new(move |e: T| Error::RuntimeError {
        message: format!("{}: {}", message, e.to_string()),
    })
}

pub fn permanent_failure<T: ToString>(e: T) -> Error {
    Error::PermanentFailure {
        message: e.to_string(),
    }
}

pub fn permanent_failure_with<T: ToString>(message: String) -> Box<dyn FnOnce(T) -> Error> {
    Box::new(move |e: T| Error::PermanentFailure {
        message: format!("{}: {}", message, e.to_string()),
    })
}

pub fn lift_invalid_input(e: Error) -> Error {
    match e {
        Error::InvalidInput { message } => Error::PermanentFailure {
            message: "InvalidInput: ".to_string() + &message,
        },
        another_error => another_error,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_map_err() {
        use std::io::{Error, ErrorKind, Result};
        let io_error: Result<()> = Err(Error::new(ErrorKind::Other, "File not found"));
        let our_error = io_error.map_err(runtime_error).unwrap_err();
        assert_eq!(our_error.to_string(), "RuntimeError: File not found");

        let io_error: Result<()> = Err(Error::new(ErrorKind::Other, "File not found"));
        let our_error = io_error
            .map_err(runtime_error_with("No backup".to_string()))
            .unwrap_err();
        assert_eq!(
            our_error.to_string(),
            "RuntimeError: No backup: File not found"
        );
    }

    #[test]
    fn test_lift_invalid_input() {
        assert_eq!(
            lift_invalid_input(invalid_input("Number must be positive")),
            permanent_failure("InvalidInput: Number must be positive")
        );
        assert_eq!(
            lift_invalid_input(runtime_error("Socket timeout")),
            runtime_error("Socket timeout")
        );
        assert_eq!(
            lift_invalid_input(permanent_failure("Devision by zero")),
            permanent_failure("Devision by zero")
        );
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, thiserror::Error)]
pub enum InitializationError {
    #[error("Failed to start async runtime: {message}")]
    AsyncRuntime { message: String },

    #[error("Failed to add a channel monitor to the chain monitor")]
    ChainMonitorWatchChannel,

    #[error("Failed to sync the chain to the chain tip")]
    ChainSync(#[from] esplora_client::Error),

    #[error("Failed to read channel monitor backup: {message}")]
    ChannelMonitorBackup { message: String },

    #[error("Failed to create esplora client: {message}")]
    EsploraClient { message: String },

    #[error("Failed to initialize keys manager: {message}")]
    KeysManager { message: String },

    #[error("Logic error: {message}")]
    Logic { message: String },

    #[error("Could not connect to peer: {message}")]
    PeerConnection { message: String },

    #[error("Failed to generate random entropy: {message}")]
    SecretGeneration { message: String },
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Failed to synchronize the blockchain")]
    ChainSync(#[from] esplora_client::Error),

    #[error("Address could not be parsed: {message}")]
    InvalidAddress { message: String },

    #[error("Pub key could not be parsed: {message}")]
    InvalidPubKey { message: String },

    #[error("Logic error: {message}")]
    Logic { message: String },

    #[error("Could not connect to peer: {message}")]
    PeerConnection { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("Grpc error")]
    Grpc,

    #[error("Network error")]
    Network,

    #[error("UnexpectedUniFFICallbackError")]
    UnexpectedUniFFI,
}
impl From<UnexpectedUniFFICallbackError> for LspError {
    fn from(_error: UnexpectedUniFFICallbackError) -> Self {
        LspError::UnexpectedUniFFI
    }
}
