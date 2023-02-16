use bitcoin::Network;

#[derive(Clone, Debug)]
pub struct TzConfig {
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}

pub struct Config {
    pub network: Network,
    pub seed: [u8; 64],
    pub esplora_api_url: String,
    pub rgs_url: String,
    pub lsp_url: String,
    pub lsp_token: String,
    pub local_persistence_path: String,
    pub timezone_config: TzConfig,
}

impl Config {
    pub fn get_seed_first_half(&self) -> [u8; 32] {
        let mut first_half = [0u8; 32];
        first_half.copy_from_slice(&self.seed[..32]);
        first_half
    }
}
