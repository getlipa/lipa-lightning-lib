#[derive(Debug, thiserror::Error)]
pub enum InitializationError {
    #[error("Failed to initialize keys manager: {message}")]
    KeysManager { message: String },
    #[error("Failed to generate random entropy: {message}")]
    SecretGeneration { message: String },
}
