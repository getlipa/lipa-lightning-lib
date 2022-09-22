use uniffi::ffi::foreigncallbacks::UnexpectedUniFFICallbackError;

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
