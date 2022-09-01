use bitcoin::Network;

/*/// An object that holds all configuration needed to start a LipaLightning instance
///
/// # Fields:
///
/// * `seed` - the seed as a byte array of length 32
/// * `electrum_url` - url for electrum connection e.g. `"tcp://electrum.blockstream.info:50001".to_string()`
/// * `ldk_peer_listening_port` - the port on which LDK listens for new p2p connections
/// * `network` - the on which to run this LipaLightning instance*/

/// An object that holds all configuration needed to start a LipaLightning instance
///
/// # Fields:
///
/// * `seed` - the seed as a byte array of length 32
/// * `bitcoind_rpc_username`
/// * `bitcoind_rpc_username`
/// * `bitcoind_rpc_port` - the port of the bitcoind RPC
/// * `bitcoind_rpc_host` - the address of the bitcoind RPC (e.g. `localhost`)
/// * `ldk_peer_listening_port` - the port on which LDK listens for new p2p connections
/// * `network` - the on which to run this LipaLightning instance
pub struct LipaLightningConfig {
    /*pub seed: Vec<u8>,
    pub electrum_url: String,
    pub ldk_peer_listening_port: u16,
    pub network: Network,*/
    pub seed: Vec<u8>,
    pub bitcoind_rpc_username: String,
    pub bitcoind_rpc_password: String,
    pub bitcoind_rpc_port: u16,
    pub bitcoind_rpc_host: String,
    pub ldk_peer_listening_port: u16,
    pub network: Network,
}
