use crate::errors::*;
use crate::types::RapidGossipSync;

use lightning_rapid_gossip_sync::GraphSyncError;
use log::info;
use reqwest::blocking::Client;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct RapidSyncClient {
    rgs_url: String,
    rapid_sync: Arc<RapidGossipSync>,
    client: Client,
}

impl RapidSyncClient {
    pub fn new(rgs_url: String, rapid_sync: Arc<RapidGossipSync>) -> LipaResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_to_permanent_failure("Failed to build HTTP client for RGS")?;
        Ok(Self {
            rgs_url,
            rapid_sync,
            client,
        })
    }

    pub fn sync(&self) -> LipaResult<()> {
        let last_sync_timestamp = self
            .rapid_sync
            .network_graph()
            .get_last_rapid_gossip_sync_timestamp()
            .unwrap_or(0);

        let snapshot_contents = self
            .client
            .get(format!("{}{}", self.rgs_url, last_sync_timestamp))
            .send()
            .map_to_runtime_error(
                RuntimeErrorCode::RgsServiceUnavailable,
                "Failed to get response from RGS server",
            )?
            .error_for_status()
            .map_to_runtime_error(
                RuntimeErrorCode::RgsServiceUnavailable,
                "The RGS server returned an error",
            )?
            .bytes()
            .map_to_runtime_error(
                RuntimeErrorCode::RgsServiceUnavailable,
                "Failed to get the RGS server response as bytes",
            )?
            .to_vec();

        match self.rapid_sync.update_network_graph(&snapshot_contents) {
            Ok(new_timestamp) => info!(
		"Successfully updated the network graph from timestamp {} to timestamp {}",
		last_sync_timestamp, new_timestamp
            ),
            Err(e) => return match e {
		GraphSyncError::DecodeError(e) => {
                    Err(e).map_to_runtime_error(RuntimeErrorCode::RgsUpdateError, "Failed to decode a network graph update")
		}
		GraphSyncError::LightningError(e) => {
                    Err(runtime_error(RuntimeErrorCode::RgsUpdateError,
				      format!("Failed to apply a network graph update to the local graph: {} - Recommended action: {:?}", e.err, e.action),
                    ))
		}
            },
	};
        Ok(())
    }
}
