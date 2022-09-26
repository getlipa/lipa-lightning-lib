use crate::errors::RuntimeError;
use crate::{AsyncRuntime, PeerManager};
use bitcoin::secp256k1::PublicKey;
use log::debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::time::sleep;

pub fn connect_to_peer(
    rt: &AsyncRuntime,
    peer_manager: Arc<PeerManager>,
    pubkey: PublicKey,
    addr: SocketAddr,
) -> Result<(), RuntimeError> {
    rt.handle().block_on(async move {
        let result =
            lightning_net_tokio::connect_outbound(Arc::clone(&peer_manager), pubkey, addr).await;

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
                        debug!("Peer connection to {} still pending", pubkey);
                    }
                }

                // Wait for the handshake to complete.
                match peer_manager
                    .get_peer_node_ids()
                    .iter()
                    .find(|id| **id == pubkey)
                {
                    Some(_) => return Ok(()),
                    None => sleep(Duration::from_millis(10)).await,
                }
            }
        }

        Err(RuntimeError::PeerConnection {
            message: format!("Failed to connect to peer {}", pubkey),
        })
    })
}
