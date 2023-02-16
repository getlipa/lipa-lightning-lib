use eel::Network;

#[derive(Debug, Clone)]
pub struct Config {
    pub network: Network,
    pub seed: Vec<u8>,
    pub esplora_api_url: String,
    pub rgs_url: String,
    pub lsp_url: String,
    pub lsp_token: String,
    pub local_persistence_path: String,
}
