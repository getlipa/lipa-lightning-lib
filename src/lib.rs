extern crate core;

pub mod callbacks;
pub mod config;
pub mod errors;
pub mod keys_manager;
pub mod secret;

mod async_runtime;
mod event_handler;
mod logger;
mod native_logger;
mod storage_persister;

use crate::async_runtime::AsyncRuntime;
use crate::callbacks::RedundantStorageCallback;
use crate::config::Config;
use crate::errors::InitializationError;
use crate::event_handler::LipaEventHandler;
use crate::keys_manager::{generate_secret, init_keys_manager};
use crate::logger::LightningLogger;
use crate::secret::Secret;
use crate::storage_persister::StoragePersister;

use bitcoin::Network;
use lightning::util::config::UserConfig;
use log::{info, warn, Level as LogLevel};

#[allow(dead_code)]
pub struct LightningNode {
    rt: AsyncRuntime,
}

#[allow(clippy::let_unit_value)]
impl LightningNode {
    pub fn new(
        config: Config,
        redundant_storage_callback: Box<dyn RedundantStorageCallback>,
    ) -> Result<Self, InitializationError> {
        let rt = AsyncRuntime::new()?;

        // Step 1. Initialize the FeeEstimator

        // Step 2. Initialize the Logger
        let _logger = LightningLogger {};

        // Step 3. Initialize the BroadcasterInterface

        // Step 4. Initialize Persist
        let persister = StoragePersister::new(redundant_storage_callback);
        if !persister.check_monitor_storage_health() {
            warn!("Monitor storage is unhealty");
        }
        if !persister.check_object_storage_health() {
            warn!("Object storage is unhealty");
        }

        // Step 5. Initialize the ChainMonitor

        // Step 6. Initialize the KeysManager
        let keys_manager =
            init_keys_manager(&config.seed).map_err(|e| InitializationError::KeysManager {
                message: e.to_string(),
            })?;

        // Step 7. Read ChannelMonitor state from disk
        let channel_monitors = persister.read_channel_monitors(&keys_manager);

        // TODO: If you are using Electrum or BIP 157/158, you must call load_outputs_to_watch
        // on each ChannelMonitor to prepare for chain synchronization in Step 9.
        for (_, _chain_monitor) in channel_monitors.iter() {
            // chain_monitor.load_outputs_to_watch(&filter);
        }

        // Step 8. Initialize the ChannelManager
        let mobile_node_user_config = build_mobile_node_user_config();
        let _channel_manager = persister.read_channel_manager(mobile_node_user_config);

        // Step 9. Sync ChannelMonitors and ChannelManager to chain tip

        // Step 10. Give ChannelMonitors to ChainMonitor

        // Step 11: Optional: Initialize the NetGraphMsgHandler
        let _graph = persister.read_graph();

        // Step 12. Initialize the PeerManager

        // Step 13. Initialize Networking

        // Step 14. Keep LDK Up-to-date with Chain Info

        // Step 15. Initialize an EventHandler
        let _event_handler = LipaEventHandler {};

        // Step 16. Initialize the ProbabilisticScorer
        let _scorer = persister.read_scorer();

        // Step 17. Initialize the InvoicePayer

        // Step 18. Initialize the Persister
        // Persister trait already implemented and instantiated ("persister")

        // Step 19. Start Background Processing

        Ok(Self { rt })
    }
}

#[allow(clippy::field_reassign_with_default)]
fn build_mobile_node_user_config() -> UserConfig {
    let mut user_config = UserConfig::default();

    // Reject any HTLCs which were to be forwarded over private channels.
    user_config.accept_forwards_to_priv_channels = false;

    // For outbound unannounced channels do not include our real on-chain channel UTXO in each invoice.
    user_config.channel_handshake_config.negotiate_scid_privacy = true;

    // Do not announce the channel publicly.
    user_config.channel_handshake_config.announced_channel = false;

    // Force an incoming channel to match our announced channel preference.
    user_config
        .channel_handshake_limits
        .force_announced_channel_preference = true;
    user_config
}

pub fn init_native_logger_once(min_level: LogLevel) {
    native_logger::init_native_logger_once(min_level);
    info!("Logger initialized");
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
