#[derive(Debug, thiserror::Error)]
pub enum InitializationError {
    #[error("Failed to start async runtime: {message}")]
    AsyncRuntime { message: String },

    #[error("Failed to add a channel monitor to the chain monitor")]
    ChainMonitorWatchChannel,

    #[error("Failed to read channel monitor backup: {message}")]
    ChannelMonitorBackup { message: String },

    #[error("Failed to create esplora client: {message}")]
    EsploraClient { message: String },

    #[error("Failed to initialize keys manager: {message}")]
    KeysManager { message: String },

    #[error("Logic error: {message}")]
    Logic { message: String },

    #[error("Failed to generate random entropy: {message}")]
    SecretGeneration { message: String },
}

#[allow(dead_code)]
pub(crate) enum ChainSyncError {
    Other,
}
