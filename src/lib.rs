#![allow(clippy::let_unit_value)]

extern crate core;

pub mod callbacks;
pub mod config;
pub mod errors;
pub mod keys_manager;
pub mod p2p_networking;
pub mod secret;

mod async_runtime;
mod chain_access;
mod confirm;
mod encryption;
mod esplora_client;
mod event_handler;
mod fee_estimator;
mod filter;
mod logger;
mod native_logger;
mod storage_persister;
mod tx_broadcaster;
mod types;

use crate::async_runtime::AsyncRuntime;
use crate::callbacks::{LspCallback, RedundantStorageCallback};
use crate::chain_access::LipaChainAccess;
use crate::config::{Config, NodeAddress};
use crate::confirm::ConfirmWrapper;
use crate::errors::{InitializationError, LipaError, LspError, RuntimeError};
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::filter::FilterImpl;
use crate::keys_manager::{
    generate_random_bytes, generate_secret, init_keys_manager, mnemonic_to_secret,
};
use crate::logger::LightningLogger;
use crate::native_logger::init_native_logger_once;
use crate::p2p_networking::{LnPeer, P2pConnection};
use crate::secret::Secret;
use crate::storage_persister::StoragePersister;
use crate::tx_broadcaster::TxBroadcaster;
use crate::types::{ChainMonitor, ChannelManager, PeerManager};

use bitcoin::blockdata::constants::genesis_block;
use bitcoin::Network;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Recipient};
use lightning::chain::{BestBlock, ChannelMonitorUpdateStatus, Watch};
use lightning::ln::channelmanager::ChainParameters;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::scoring::{ProbabilisticScorer, ProbabilisticScoringParameters};
use lightning::util::config::UserConfig;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_rapid_gossip_sync::RapidGossipSync;
use log::{debug, error, warn, Level as LogLevel};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};

#[allow(dead_code)]
pub struct LightningNode {
    #[allow(dead_code)]
    rt: AsyncRuntime,
    esplora_client: Arc<EsploraClient>,
    background_processor: BackgroundProcessor,
    channel_manager: Arc<ChannelManager>,
    peer_manager: Arc<PeerManager>,
    p2p_connector_handle: JoinHandle<()>,
    sync_handle: JoinHandle<()>,
}

impl LightningNode {
    #[allow(clippy::result_large_err)]
    pub fn new(
        config: &Config,
        redundant_storage_callback: Box<dyn RedundantStorageCallback>,
    ) -> Result<Self, InitializationError> {
        let rt = AsyncRuntime::new()?;
        let genesis_hash = genesis_block(config.network).header.block_hash();

        let esplora_client = Arc::new(EsploraClient::new(&config.esplora_api_url.clone())?);

        // Step 1. Initialize the FeeEstimator
        let fee_estimator = Arc::new(FeeEstimator {});

        // Step 2. Initialize the Logger
        let logger = Arc::new(LightningLogger {});

        // Step 3. Initialize the BroadcasterInterface
        let tx_broadcaster = Arc::new(TxBroadcaster::new(Arc::clone(&esplora_client)));

        // Step 4. Initialize Persist
        let persister = Arc::new(StoragePersister::new(redundant_storage_callback));
        if !persister.check_monitor_storage_health() {
            warn!("Monitor storage is unhealty");
        }
        if !persister.check_object_storage_health() {
            warn!("Object storage is unhealty");
        }

        // Step x. Initialize the Transaction Filter
        let filter = Arc::new(FilterImpl::new());

        // Step 5. Initialize the ChainMonitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            Some(Arc::clone(&filter)),
            Arc::clone(&tx_broadcaster),
            Arc::clone(&logger),
            Arc::clone(&fee_estimator),
            Arc::clone(&persister),
        ));

        // Step 6. Initialize the KeysManager
        let keys_manager = Arc::new(init_keys_manager(&config.seed).map_err(|e| {
            InitializationError::KeysManager {
                message: e.to_string(),
            }
        })?);

        // Step 7. Read ChannelMonitor state from disk
        let mut channel_monitors = persister.read_channel_monitors(&*keys_manager);

        // If you are using Electrum or BIP 157/158, you must call load_outputs_to_watch
        // on each ChannelMonitor to prepare for chain synchronization in Step 9.
        for (_, channel_monitor) in channel_monitors.iter() {
            channel_monitor.load_outputs_to_watch(&filter);
        }

        // Step 8. Initialize the ChannelManager
        let mobile_node_user_config = build_mobile_node_user_config();
        // TODO: Init properly.
        let best_block = BestBlock::from_genesis(config.network);
        let chain_params = ChainParameters {
            network: config.network,
            best_block,
        };
        let mut_channel_monitors: Vec<&mut ChannelMonitor<InMemorySigner>> =
            channel_monitors.iter_mut().map(|(_, m)| m).collect();

        let (channel_manager_block_hash, channel_manager) = persister
            .read_or_init_channel_manager(
                Arc::clone(&chain_monitor),
                Arc::clone(&tx_broadcaster),
                Arc::clone(&keys_manager),
                Arc::clone(&fee_estimator),
                Arc::clone(&logger),
                mut_channel_monitors,
                mobile_node_user_config,
                chain_params,
            )?;
        let channel_manager = Arc::new(channel_manager);

        // Step 9. Sync ChannelMonitors and ChannelManager to chain tip
        let confirm = ConfirmWrapper::new(vec![&*channel_manager, &*chain_monitor]);
        let chain_access = Arc::new(Mutex::new(LipaChainAccess::new(
            Arc::clone(&esplora_client),
            filter,
            channel_manager_block_hash.unwrap_or(genesis_hash),
        )));
        chain_access.lock().unwrap().sync(&confirm)?;

        // Step 10. Give ChannelMonitors to ChainMonitor
        for (_, channel_monitor) in channel_monitors {
            let funding_outpoint = channel_monitor.get_funding_txo().0;
            match chain_monitor.watch_channel(funding_outpoint, channel_monitor) {
                ChannelMonitorUpdateStatus::Completed => {}
                ChannelMonitorUpdateStatus::InProgress => {
                    return Err(InitializationError::ChainMonitorWatchChannel)
                }
                ChannelMonitorUpdateStatus::PermanentFailure => {
                    return Err(InitializationError::ChainMonitorWatchChannel)
                }
            }
        }

        // Step 11: Optional: Initialize the NetGraphMsgHandler
        let _graph = persister.read_graph();
        let graph = Arc::new(NetworkGraph::new(genesis_hash, Arc::clone(&logger)));
        let rapid_gossip = Arc::new(RapidGossipSync::new(Arc::clone(&graph)));

        // Step 12. Initialize the PeerManager
        let peer_manager = Arc::new(init_peer_manager(
            Arc::clone(&channel_manager),
            &keys_manager,
            Arc::clone(&logger),
        )?);

        // Step 13. Initialize Networking
        let p2p_connector_handle =
            P2pConnection::init_background_task(&config.lsp_node, rt.handle(), &peer_manager)
                .unwrap(); // todo proper error handling instead of unwrap()

        // Step 14. Keep LDK Up-to-date with Chain Info
        // TODO: optimize how often we want to run sync. LDK-sample syncs every second and
        //       LDKLite syncs every 5 seconds. Let's try 5 seconds first and change if needed
        let channel_manager_regular_sync = Arc::clone(&channel_manager);
        let chain_monitor_regular_sync = Arc::clone(&chain_monitor);
        let sync_handle = rt
            .handle()
            .spawn_repeating_task(Duration::from_secs(5), move || {
                let chain_access_regular_sync = Arc::clone(&chain_access);
                let channel_manager_regular_sync = Arc::clone(&channel_manager_regular_sync);
                let chain_monitor_regular_sync = Arc::clone(&chain_monitor_regular_sync);
                async move {
                    let confirm_regular_sync = ConfirmWrapper::new(vec![
                        &*channel_manager_regular_sync,
                        &*chain_monitor_regular_sync,
                    ]);
                    let now = Instant::now();
                    match chain_access_regular_sync
                        .lock()
                        .unwrap()
                        .sync(&confirm_regular_sync)
                    {
                        Ok(_) => debug!(
                            "Sync to blockchain finished in {}ms",
                            now.elapsed().as_millis()
                        ),
                        Err(e) => error!("Sync to blockchain failed: {:?}", e),
                    }
                }
            });

        // Step 15. Initialize an EventHandler
        let event_handler = Arc::new(LipaEventHandler {});

        // Step 16. Initialize the ProbabilisticScorer
        let _scorer = persister.read_scorer();
        let scorer = Arc::new(Mutex::new(ProbabilisticScorer::new(
            ProbabilisticScoringParameters::default(),
            Arc::clone(&graph),
            Arc::clone(&logger),
        )));

        // Step 17. Initialize the InvoicePayer

        // Step 18. Initialize the Persister
        // Persister trait already implemented and instantiated ("persister")

        // Step 19. Start Background Processing
        // The fact that we do not restart the background process assumes that
        // it will never fail. However it may fail:
        //  1. on persisting channel manager, but it never fails since we ignore
        //     such failures in StoragePersister::persist_manager()
        //  2. on persisting scorer or network graph on exit, but we do not care
        // The other strategy to handle errors and restart the process will be
        // more difficult but will not provide any benefits.
        let background_processor = BackgroundProcessor::start(
            persister,
            event_handler,
            chain_monitor,
            Arc::clone(&channel_manager),
            GossipSync::rapid(rapid_gossip),
            Arc::clone(&peer_manager),
            logger,
            Some(scorer),
        );

        Ok(Self {
            rt,
            esplora_client,
            background_processor,
            channel_manager: Arc::clone(&channel_manager),
            peer_manager,
            p2p_connector_handle,
            sync_handle,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        let chans = self.channel_manager.list_channels();
        let local_balance_msat = chans.iter().map(|c| c.balance_msat).sum::<u64>();
        NodeInfo {
            node_pubkey: self.channel_manager.get_our_node_id().serialize().to_vec(),
            num_channels: chans.len() as u16,
            num_usable_channels: chans.iter().filter(|c| c.is_usable).count() as u16,
            local_balance_msat,
            num_peers: self.peer_manager.get_peer_node_ids().len() as u16,
        }
    }

    pub fn connected_to_node(&self, lsp_node: &NodeAddress) -> bool {
        let peer = Arc::new(LnPeer::try_from(lsp_node).unwrap()); // todo proper error handling instead of unwrap()
        self.peer_manager
            .get_peer_node_ids()
            .contains(&peer.pub_key)
    }
}

impl Drop for LightningNode {
    fn drop(&mut self) {
        self.p2p_connector_handle.abort();
        self.sync_handle.abort();

        // TODO: Stop reconnecting to peers
        self.peer_manager.disconnect_all_peers();

        // The background processor implements the drop trait itself.
        // It therefore doesn't have to be stopped manually.
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

#[allow(clippy::result_large_err)]
fn init_peer_manager(
    channel_manager: Arc<ChannelManager>,
    keys_manager: &KeysManager,
    logger: Arc<LightningLogger>,
) -> Result<PeerManager, InitializationError> {
    let ephemeral_bytes = generate_random_bytes()?;
    let our_node_secret = keys_manager
        .get_node_secret(Recipient::Node)
        .map_err(|()| InitializationError::Logic {
            message: "Get node secret for node recipient failed".to_string(),
        })?;
    Ok(PeerManager::new_channel_only(
        channel_manager,
        IgnoringMessageHandler {},
        our_node_secret,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32,
        &ephemeral_bytes,
        logger,
    ))
}

pub struct NodeInfo {
    pub node_pubkey: Vec<u8>,
    pub num_channels: u16,
    pub num_usable_channels: u16,
    pub local_balance_msat: u64,
    pub num_peers: u16,
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
