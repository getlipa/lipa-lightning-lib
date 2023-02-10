use bitcoin::Network;

pub struct Config {
    pub network: Network,
    pub seed: [u8; 64],
    pub esplora_api_url: String,
    pub rgs_url: String,
    pub lsp_url: String,
    pub lsp_token: String,
    pub local_persistence_path: String,
}

impl Config {
    pub fn get_seed_first_half(&self) -> [u8; 32] {
        let mut first_half = [0u8; 32];
        first_half.copy_from_slice(&self.seed[..32]);
        first_half
    }
}
