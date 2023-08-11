use crate::errors::*;
use crate::types::RapidGossipSync;

use log::info;
use perro::{runtime_error, MapToError};
use reqwest::blocking::Client;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) struct RapidSyncClient {
    rgs_url: String,
    rapid_sync: Arc<RapidGossipSync>,
    client: Client,
}

const SECONDS_IN_ONE_DAY: u64 = 60 * 60 * 24;

impl RapidSyncClient {
    pub fn new(rgs_url: String, rapid_sync: Arc<RapidGossipSync>) -> Result<Self> {
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

    pub fn sync(&self) -> InternalResult<()> {
        let mut last_sync_timestamp = self
            .rapid_sync
            .network_graph()
            .get_last_rapid_gossip_sync_timestamp()
            .unwrap_or(0);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_to_permanent_failure("Current system time is before unix epoch")?
            .as_secs();

        if now - last_sync_timestamp as u64 >= SECONDS_IN_ONE_DAY {
            info!("Last sync ({last_sync_timestamp}) older than one day, forcefully updating the network graph from scratch");
            last_sync_timestamp = 0;
        }

        let snapshot_contents = self
            .client
            .get(format!("{}{}", self.rgs_url, last_sync_timestamp))
            .send()
            .map_to_runtime_error(
                InternalRuntimeErrorCode::RgsServiceUnavailable,
                "Failed to get response from RGS server",
            )?
            .error_for_status()
            .map_to_runtime_error(
                InternalRuntimeErrorCode::RgsServiceUnavailable,
                "The RGS server returned an error",
            )?
            .bytes()
            .map_to_runtime_error(
                InternalRuntimeErrorCode::RgsServiceUnavailable,
                "Failed to get the RGS server response as bytes",
            )?
            .to_vec();

        let new_timestamp = self
            .rapid_sync
            .update_network_graph(&snapshot_contents)
            .map_err(|e| {
                runtime_error(
                    InternalRuntimeErrorCode::RgsUpdateError,
                    format!("Failed to apply network graph update: {e:?}"),
                )
            })?;
        info!("Successfully updated the network graph from timestamp {last_sync_timestamp} to timestamp {new_timestamp}");
        Ok(())
    }
}
