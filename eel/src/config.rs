use bitcoin::Network;

pub struct Config {
    pub network: Network,
    pub seed: Vec<u8>,
    pub esplora_api_url: String,
    pub rgs_url: String,
    pub local_persistence_path: String,
}
