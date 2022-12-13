use bitcoin::Network;

/// An object that holds all configuration needed to start a LightningNode instance.
///
/// # Fields:
///
/// * `network` - the Bitcoin Network the node should run on: Bitcoin | Testnet | Signet | Regtest
/// * `seed` - the seed derived from the mnemonic and optional pass phrase.
/// * `esplora_api_url` - url of the esplora API to retrieve chain data from and over which transactions are being published - e.g. "https://blockstream.info/testnet/api"
pub struct Config {
    pub network: Network,
    pub seed: Vec<u8>,
    pub esplora_api_url: String,
    pub lsp_node: NodeAddress,
    pub rgs_url: String,
}

#[derive(Debug, Clone)]
pub struct NodeAddress {
    pub pub_key: String,
    pub host: String,
}
