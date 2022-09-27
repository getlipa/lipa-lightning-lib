use crate::errors::RuntimeError;
use crate::{NodeAddress, PeerManager};
use bitcoin::secp256k1::PublicKey;
use log::debug;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::time::sleep;

pub(crate) struct P2pConnections {}

impl P2pConnections {
    pub async fn connect_peer(
        peer: &NodeAddress,
        peer_manager: Arc<PeerManager>,
    ) -> Result<(), RuntimeError> {
        let peer = LnPeer::try_from(peer)?;

        if Self::is_connected(&peer, Arc::clone(&peer_manager)) {
            debug!("Peer {} is already connected", peer.pub_key);
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
                if Self::is_connected(&peer, Arc::clone(&peer_manager)) {
                    debug!("Peer connection to {} established", peer.pub_key);
                    return Ok(());
                } else {
                    sleep(Duration::from_millis(10)).await;
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
            .find(|id| *id == &peer.pub_key)
            .is_some()
    }
}

struct LnPeer {
    pub_key: PublicKey,
    address: SocketAddr,
}

impl TryFrom<&NodeAddress> for LnPeer {
    type Error = RuntimeError;

    fn try_from(node_address: &NodeAddress) -> Result<Self, Self::Error> {
        let pub_key = PublicKey::from_str(&node_address.pub_key).map_err(|e| {
            RuntimeError::InvalidPubKey {
                message: e.to_string(),
            }
        })?;
        let address = SocketAddr::from_str(&node_address.address).map_err(|e| {
            RuntimeError::InvalidAddress {
                message: e.to_string(),
            }
        })?;

        Ok(Self { pub_key, address })
    }
}
