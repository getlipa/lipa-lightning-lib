use bitcoin::Network;

pub struct Config {
    pub network: Network,
    pub seed: Vec<u8>,
    pub esplora_api_url: String,
    pub lsp_node: NodeAddress,
    pub rgs_url: String,
    pub local_persistence_path: String,
}

#[derive(Debug, Clone)]
pub struct NodeAddress {
    pub pub_key: String,
    pub host: String,
}
