use crate::async_runtime::{Handle, RepeatingTaskHandle};
use crate::lsp::{LspClient, LspInfo};
use crate::p2p_networking::{connect_peer, LnPeer};
use crate::types::PeerManager;

use log::{debug, error};
use std::sync::{Arc, Mutex};
use tokio::time::Duration;

pub(crate) struct TaskPeriods {
    pub update_lsp_info: Option<Duration>,
    pub reconnect_to_lsp: Duration,
}

pub(crate) struct TaskManager {
    runtime_handle: Handle,
    lsp_client: Arc<LspClient>,
    peer_manager: Arc<PeerManager>,

    lsp_info: Arc<Mutex<Option<LspInfo>>>,

    task_handles: Vec<RepeatingTaskHandle>,
}

impl TaskManager {
    pub fn new(
        runtime_handle: Handle,
        lsp_client: Arc<LspClient>,
        peer_manager: Arc<PeerManager>,
    ) -> Self {
        Self {
            runtime_handle,
            lsp_client,
            peer_manager,
            lsp_info: Arc::new(Mutex::new(None)),
            task_handles: Vec::new(),
        }
    }

    pub fn get_lsp_info(&self) -> Option<LspInfo> {
        (*self.lsp_info.lock().unwrap()).clone()
    }

    pub fn request_shutdowns(&mut self) {
        self.task_handles
            .drain(..)
            .for_each(|h| h.request_shutdown());
    }

    pub fn restart(&mut self, periods: TaskPeriods) {
        self.request_shutdowns();

        // TODO: Blockchain sync.

        // TODO: Fee estimator.

        // LSP info update.
        if let Some(period) = periods.update_lsp_info {
            self.task_handles.push(self.start_lsp_info_update(period));
        }

        // Reconnect to LSP LN node.
        self.task_handles
            .push(self.start_reconnect_to_lsp(periods.reconnect_to_lsp));

        // TODO: Update network graph.

        // TODO: Reconnect to channels' peers.
    }

    fn start_lsp_info_update(&self, period: Duration) -> RepeatingTaskHandle {
        let peer_manager = Arc::clone(&self.peer_manager);
        let lsp_client = Arc::clone(&self.lsp_client);
        let lsp_info = Arc::clone(&self.lsp_info);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let peer_manager = Arc::clone(&peer_manager);
            let lsp_client = Arc::clone(&lsp_client);
            let lsp_info = Arc::clone(&lsp_info);
            async move {
                let result = tokio::task::spawn_blocking(move || lsp_client.query_info()).await;
                match result {
                    Ok(Ok(new_lsp_info)) => {
                        debug!("New LSP info: {:?}", new_lsp_info);
                        *lsp_info.lock().unwrap() = Some(new_lsp_info.clone());

                        // Kick in reconnecting to LSP when we get new info.
                        let peer = LnPeer {
                            pub_key: new_lsp_info.node_info.pubkey,
                            host: new_lsp_info.node_info.address,
                        };
                        if let Err(e) = connect_peer(&peer, peer_manager).await {
                            error!("Connecting to peer {} failed: {}", peer, e);
                        }
                    }
                    Ok(Err(e)) => error!("Failed to query LSP: {}", e),
                    Err(e) => error!("Query LSP task panicked: {}", e),
                }
            }
        })
    }

    fn start_reconnect_to_lsp(&self, period: Duration) -> RepeatingTaskHandle {
        let peer_manager = Arc::clone(&self.peer_manager);
        let lsp_info = Arc::clone(&self.lsp_info);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let peer_manager = Arc::clone(&peer_manager);
            let lsp_info = Arc::clone(&lsp_info);
            async move {
                let lsp_info = (*lsp_info.lock().unwrap()).clone();
                if let Some(lsp_info) = lsp_info {
                    let peer = LnPeer {
                        pub_key: lsp_info.node_info.pubkey,
                        host: lsp_info.node_info.address,
                    };
                    if let Err(e) = connect_peer(&peer, peer_manager).await {
                        error!("Connecting to peer {} failed: {}", peer, e);
                    }
                }
            }
        })
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.request_shutdowns();
    }
}
