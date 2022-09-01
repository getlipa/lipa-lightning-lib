#[derive(Debug, thiserror::Error)]
pub enum LipaLightningError {
    #[error("Here we can have an error message with values {a} and {b}")]
    Placeholder { a: u64, b: u64 },
    #[error("Failed to connect to peer {peer_id} on address {peer_addr}")]
    PeerConnection { peer_id: String, peer_addr: String },
    #[error("Failed to open channel with node {peer_id} despite it already being our peer")]
    ChannelOpen { peer_id: String },
    #[error("Failed to parse the provided BOLT11 invoice")]
    InvoiceParsing,
    #[error("The provided BOLT11 invoice is not valid: {info}")]
    InvoiceInvalid { info: String },
    #[error("Failed to find route: {info}")]
    Routing { info: String },
    #[error("Failed to send payment: {info}")]
    PaymentFail { info: String },
    #[error("3L internal error: {info}")]
    InternalError { info: String },
    #[error("Failed to parse PublicKey {pubkey}")]
    PubkeyParsing { pubkey: String },
    #[error("The payee is invalid: {info}")]
    InvalidPayee { info: String },
    #[error("Failed to create invoice: {info}")]
    InvoiceCreation { info: String },
}
