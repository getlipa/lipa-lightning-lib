use crate::errors::{runtime_error, LipaError, LipaResult, MapToLipaError, RuntimeErrorCode};
use crate::{NodeAddress, PeerManager};
use bitcoin::secp256k1::PublicKey;
use log::{debug, trace};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::time::sleep;

pub(crate) async fn connect_peer(peer: &LnPeer, peer_manager: Arc<PeerManager>) -> LipaResult<()> {
    if is_connected(peer, &peer_manager) {
        trace!("Peer {} is already connected", peer.pub_key);
        return Ok(());
    }

    debug!("Connecting to peer {} ...", peer.pub_key);
    let connection_closed_future =
        lightning_net_tokio::connect_outbound(Arc::clone(&peer_manager), peer.pub_key, peer.host)
            .await
            .ok_or_else(|| {
                runtime_error(
                    RuntimeErrorCode::GenericError,
                    "Failed to establish TCP connection",
                )
            })?;
    let mut connection_closed_future = Box::pin(connection_closed_future);
    debug!("TCP connection to peer {} established", peer.pub_key);

    // Wait for LN handshake to complete.
    while !is_connected(peer, &peer_manager) {
        if let Poll::Ready(()) = futures::poll!(&mut connection_closed_future) {
            return Err(runtime_error(
                RuntimeErrorCode::GenericError,
                "Peer disconnected before LN handshake completed",
            ));
        }

        debug!("LN handshake to peer {} still pending ...", peer.pub_key);
        sleep(Duration::from_millis(10)).await;
    }

    debug!("LN connection to peer {} established", peer.pub_key);
    Ok(())
}

fn is_connected(peer: &LnPeer, peer_manager: &PeerManager) -> bool {
    peer_manager
        .get_peer_node_ids()
        .iter()
        .any(|id| *id == peer.pub_key)
}

pub(crate) struct LnPeer {
    pub pub_key: PublicKey,
    pub host: SocketAddr,
}

impl TryFrom<&NodeAddress> for LnPeer {
    type Error = LipaError;

    fn try_from(node_address: &NodeAddress) -> LipaResult<Self> {
        let pub_key = PublicKey::from_str(&node_address.pub_key)
            .map_to_invalid_input("Invalid node public key")?;
        let host = SocketAddr::from_str(&node_address.host)
            .map_to_invalid_input("Invalid node address")?;

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
