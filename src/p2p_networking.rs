use crate::async_runtime::Handle;
use crate::errors::{LipaError, LipaResult, MapToLipaError, RuntimeError};
use crate::{NodeAddress, PeerManager};
use bitcoin::secp256k1::PublicKey;
use log::{debug, error, trace};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::sleep;

pub(crate) struct P2pConnections {}

impl P2pConnections {
    pub fn run_bg_connector(
        peer: &NodeAddress,
        handle1: Handle,
        peer_mgr: &Arc<PeerManager>,
    ) -> LipaResult<JoinHandle<()>> {
        let peer = Arc::new(LnPeer::try_from(peer)?);
        let peer_mgr_clone = peer_mgr.clone();

        let handle = handle1.spawn_repeating_task(Duration::from_secs(1), move || {
            let peer_clone = Arc::clone(&peer);
            let peer_manager = Arc::clone(&peer_mgr_clone);
            async move {
                if let Err(e) = P2pConnections::connect_peer(&peer_clone, peer_manager).await {
                    error!(
                        "Connecting to peer {} failed with error: {:?}",
                        peer_clone, e
                    )
                }
            }
        });

        Ok(handle)
    }

    async fn connect_peer(
        peer: &LnPeer,
        peer_manager: Arc<PeerManager>,
    ) -> Result<(), RuntimeError> {
        if Self::is_connected(peer, Arc::clone(&peer_manager)) {
            trace!("Peer {} is already connected", peer.pub_key);
            return Ok(());
        }

        let result = lightning_net_tokio::connect_outbound(
            Arc::clone(&peer_manager),
            peer.pub_key,
            peer.address,
        )
        .await;

        if let Some(connection_closed_future) = result {
            let mut connection_closed_future = Box::pin(connection_closed_future);
            loop {
                // Make sure the connection is still established.
                match futures::poll!(&mut connection_closed_future) {
                    Poll::Ready(_) => {
                        return Err(RuntimeError::PeerConnection {
                            message: "Peer disconnected before handshake completed".to_string(),
                        });
                    }
                    Poll::Pending => {
                        debug!("Peer connection to {} still pending", peer.pub_key);
                    }
                }

                // Wait for the handshake to complete.
                if Self::is_connected(peer, Arc::clone(&peer_manager)) {
                    debug!("Peer connection to {} established", peer.pub_key);
                    return Ok(());
                } else {
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }

        Err(RuntimeError::PeerConnection {
            message: format!("Failed to connect to peer {}", peer.pub_key),
        })
    }

    fn is_connected(peer: &LnPeer, peer_manager: Arc<PeerManager>) -> bool {
        peer_manager
            .get_peer_node_ids()
            .iter()
            .any(|id| *id == peer.pub_key)
    }
}

pub struct LnPeer {
    pub pub_key: PublicKey,
    pub address: SocketAddr,
}

impl TryFrom<&NodeAddress> for LnPeer {
    type Error = LipaError;

    fn try_from(node_address: &NodeAddress) -> LipaResult<Self> {
        let pub_key = PublicKey::from_str(&node_address.pub_key)
            .map_to_invalid_input("Could not parse node public key")?;
        let address = SocketAddr::from_str(&node_address.address)
            .map_to_invalid_input("Could not parse node address")?;

        Ok(Self { pub_key, address })
    }
}

impl fmt::Display for LnPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pub_key, self.address)
    }
}

#[cfg(test)]
mod test {
    use crate::config::NodeAddress;
    use crate::p2p_networking::LnPeer;

    #[test]
    fn test_conversion_from_strings() {
        let sample_pubkey = "03beb9d00217e9cf9d485e47ffc6e6842c79d8941a755e261a796fe0c2e7ba2e53";
        let sample_address = "1.2.3.4:9735";

        let sample_node = NodeAddress {
            pub_key: sample_pubkey.to_string(),
            address: sample_address.to_string(),
        };

        let ln_peer = LnPeer::try_from(&sample_node).unwrap();

        assert_eq!(
            ln_peer.to_string(),
            format!("{}@{}", sample_pubkey, sample_address)
        );
    }
}
