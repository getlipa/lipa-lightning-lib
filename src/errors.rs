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

pub fn invalid_input<E: ToString>(e: E) -> Error {
    Error::InvalidInput {
        message: e.to_string(),
    }
}

pub fn invalid_input_with<M: ToString + 'static, E: ToString>(
    message: M,
) -> Box<dyn FnOnce(E) -> Error> {
    Box::new(move |e: E| Error::InvalidInput {
        message: format!("{}: {}", message.to_string(), e.to_string()),
    })
}

pub fn runtime_error<E: ToString>(e: E) -> Error {
    Error::RuntimeError {
        message: e.to_string(),
    }
}

pub fn runtime_error_with<M: ToString + 'static, E: ToString>(
    message: M,
) -> Box<dyn FnOnce(E) -> Error> {
    Box::new(move |e: E| Error::RuntimeError {
        message: format!("{}: {}", message.to_string(), e.to_string()),
    })
}

pub fn permanent_failure<E: ToString>(e: E) -> Error {
    Error::PermanentFailure {
        message: e.to_string(),
    }
}

pub fn permanent_failure_with<M: ToString + 'static, E: ToString>(
    message: M,
) -> Box<dyn FnOnce(E) -> Error> {
    Box::new(move |e: E| Error::PermanentFailure {
        message: format!("{}: {}", message.to_string(), e.to_string()),
    })
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait LipaResult<T> {
    fn lift_invalid_input(self) -> Result<T>;
    fn prefix_error<M: ToString + 'static>(self, message: M) -> Result<T>;
}

impl<T> LipaResult<T> for Result<T> {
    fn lift_invalid_input(self) -> Result<T> {
        self.map_err(|e| match e {
            Error::InvalidInput { message } => Error::PermanentFailure {
                message: format!("InvalidInput: {}", message),
            },
            another_error => another_error,
        })
    }

    fn prefix_error<M: ToString + 'static>(self, prefix: M) -> Result<T> {
        self.map_err(|e| match e {
            Error::InvalidInput { message } => Error::InvalidInput {
                message: format!("{}: {}", prefix.to_string(), message),
            },
            Error::RuntimeError { message } => Error::RuntimeError {
                message: format!("{}: {}", prefix.to_string(), message),
            },
            Error::PermanentFailure { message } => Error::PermanentFailure {
                message: format!("{}: {}", prefix.to_string(), message),
            },
        })
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
            .map_err(runtime_error_with("No backup"))
            .unwrap_err();
        assert_eq!(
            our_error.to_string(),
            "RuntimeError: No backup: File not found"
        );
    }

    #[test]
    fn test_lift_invalid_input() {
        let result: Result<()> = Err(invalid_input("Number must be positive")).lift_invalid_input();
        assert_eq!(
            result.unwrap_err().to_string(),
            "PermanentFailure: InvalidInput: Number must be positive"
        );

        let result: Result<()> = Err(runtime_error("Socket timeout")).lift_invalid_input();
        assert_eq!(
            result.unwrap_err().to_string(),
            "RuntimeError: Socket timeout"
        );

        let result: Result<()> = Err(permanent_failure("Devision by zero")).lift_invalid_input();
        assert_eq!(
            result.unwrap_err().to_string(),
            "PermanentFailure: Devision by zero"
        );
    }

    #[test]
    fn test_prefix_error() {
        let result: Result<()> =
            Err(invalid_input("Number must be positive")).prefix_error("Invalid amount");
        assert_eq!(
            result.unwrap_err().to_string(),
            "InvalidInput: Invalid amount: Number must be positive"
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
