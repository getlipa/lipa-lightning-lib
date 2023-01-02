use crate::async_runtime::Handle;
use crate::errors::{runtime_error, LipaError, LipaResult, MapToLipaError, RuntimeErrorCode};
use crate::{NodeAddress, PeerManager, RepeatingTaskHandle};
use bitcoin::secp256k1::PublicKey;
use log::{debug, error, trace};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::time::sleep;

pub(crate) struct P2pConnection {}

impl P2pConnection {
    pub fn init_background_task(
        peer: &NodeAddress,
        runtime_handle: Handle,
        peer_manager: &Arc<PeerManager>,
    ) -> LipaResult<RepeatingTaskHandle> {
        let peer = Arc::new(LnPeer::try_from(peer)?);
        let peer_manager_clone = Arc::clone(peer_manager);

        let join_handle = runtime_handle.spawn_repeating_task(Duration::from_secs(1), move || {
            let peer_clone = Arc::clone(&peer);
            let peer_manager = Arc::clone(&peer_manager_clone);
            async move {
                if let Err(e) = P2pConnection::connect_peer(&peer_clone, peer_manager).await {
                    error!(
                        "Connecting to peer {} failed with error: {:?}",
                        peer_clone, e
                    )
                }
            }
        });

        Ok(join_handle)
    }

    async fn connect_peer(peer: &LnPeer, peer_manager: Arc<PeerManager>) -> LipaResult<()> {
        if Self::is_connected(peer, Arc::clone(&peer_manager)) {
            trace!("Peer {} is already connected", peer.pub_key);
            return Ok(());
        }

        let result = lightning_net_tokio::connect_outbound(
            Arc::clone(&peer_manager),
            peer.pub_key,
            peer.host,
        )
        .await;

        if let Some(connection_closed_future) = result {
            let mut connection_closed_future = Box::pin(connection_closed_future);
            loop {
                // Make sure the connection is still established.
                match futures::poll!(&mut connection_closed_future) {
                    Poll::Ready(_) => {
                        return Err(runtime_error(
                            RuntimeErrorCode::GenericError,
                            "Peer disconnected before handshake completed",
                        ));
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

        Err(runtime_error(
            RuntimeErrorCode::GenericError,
            format!("Failed to connect to peer {}", peer.pub_key),
        ))
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
    pub host: SocketAddr,
}

impl TryFrom<&NodeAddress> for LnPeer {
    type Error = LipaError;

    fn try_from(node_address: &NodeAddress) -> LipaResult<Self> {
        let pub_key = PublicKey::from_str(&node_address.pub_key)
            .map_to_invalid_input("Could not parse node public key")?;
        let host = SocketAddr::from_str(&node_address.host)
            .map_to_invalid_input("Could not parse node address")?;

        Ok(Self { pub_key, host })
    }
}

impl fmt::Display for LnPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pub_key, self.host)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::NodeAddress;
    use crate::p2p_networking::LnPeer;

    #[test]
    fn test_conversion_from_strings() {
        let sample_pubkey = "03beb9d00217e9cf9d485e47ffc6e6842c79d8941a755e261a796fe0c2e7ba2e53";
        let sample_address = "1.2.3.4:9735";

        let sample_node = NodeAddress {
            pub_key: sample_pubkey.to_string(),
            host: sample_address.to_string(),
        };

        let ln_peer = LnPeer::try_from(&sample_node).unwrap();

        assert_eq!(
            ln_peer.to_string(),
            format!("{}@{}", sample_pubkey, sample_address)
        );
    }
}
