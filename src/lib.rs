extern crate core;

pub mod callbacks;
pub mod config;
pub mod errors;
pub mod keys_manager;

mod logger;
mod native_logger;
mod storage_persister;

use crate::callbacks::RedundantStorageCallback;
use crate::config::Config;
use crate::errors::InitializationError;
use crate::keys_manager::{generate_secret_seed, init_keys_manager};
use crate::logger::LightningLogger;
use crate::storage_persister::StoragePersister;
use log::{info, warn, Level as LogLevel};

pub struct LightningNode;

impl LightningNode {
    pub fn new(
        config: Config,
        redundant_storage_callback: Box<dyn RedundantStorageCallback>,
    ) -> Result<Self, InitializationError> {
        // Step 1. Initialize the FeeEstimator

        // Step 2. Initialize the Logger
        let _logger = LightningLogger {};

        // Step 3. Initialize the BroadcasterInterface

        // Step 4. Initialize Persist
        let persister = StoragePersister::new(redundant_storage_callback);
        if !persister.check_monitor_storage_health() {
            warn!("Monitor storage is unhealty");
        }

        // Step 5. Initialize the ChainMonitor

        // Step 6. Initialize the KeysManager
        let _keys_manager = init_keys_manager(&config.secret_seed).map_err(|e| {
            InitializationError::KeysManager {
                message: e.to_string(),
            }
        })?;

        // Step 7. Read ChannelMonitor state from disk
        let _channel_monitors = persister.read_channel_monitors();

        // Step 8. Initialize the ChannelManager

        // Step 9. Sync ChannelMonitors and ChannelManager to chain tip

        // Step 10. Give ChannelMonitors to ChainMonitor

        // Step 11: Optional: Initialize the NetGraphMsgHandler

        // Step 12. Initialize the PeerManager

        // Step 13. Initialize Networking

        // Step 14. Keep LDK Up-to-date with Chain Info

        // Step 15. Initialize an EventHandler

        // Step 16. Initialize the ProbabilisticScorer

        // Step 17. Initialize the InvoicePayer

        // Step 18. Initialize the Persister

        // Step 19. Start Background Processing

        Ok(Self {})
    }
}

pub fn init_native_logger_once(min_level: LogLevel) {
    native_logger::init_native_logger_once(min_level);
    info!("Logger initialized");
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
