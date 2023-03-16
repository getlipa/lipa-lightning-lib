use crate::errors::{Result, RuntimeErrorCode};
use crate::PeerManager;
use bitcoin::secp256k1::PublicKey;
use log::{debug, trace};
use perro::{runtime_error, OptionToError};
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::time::sleep;

pub(crate) async fn connect_peer(peer: &LnPeer, peer_manager: Arc<PeerManager>) -> Result<()> {
    if is_connected(peer, &peer_manager) {
        trace!("Peer {} is already connected", peer.pub_key);
        return Ok(());
    }

    debug!("Connecting to peer {} ...", peer.pub_key);
    let connection_closed_future =
        lightning_net_tokio::connect_outbound(Arc::clone(&peer_manager), peer.pub_key, peer.host)
            .await
            .ok_or_runtime_error(
                RuntimeErrorCode::GenericError,
                "Failed to establish TCP connection",
            )?;
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
        sleep(Duration::from_millis(100)).await;
    }

    debug!("LN connection to peer {} established", peer.pub_key);
    Ok(())
}

fn is_connected(peer: &LnPeer, peer_manager: &PeerManager) -> bool {
    peer_manager
        .get_peer_node_ids()
        .iter()
        .any(|(id, _net_address)| *id == peer.pub_key)
}

pub(crate) struct LnPeer {
    pub pub_key: PublicKey,
    pub host: SocketAddr,
}

impl fmt::Display for LnPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pub_key, self.host)
    }
}
