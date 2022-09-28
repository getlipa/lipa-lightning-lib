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
mod event_handler;
mod fee_estimator;
mod logger;
mod native_logger;
mod storage_persister;
mod tx_broadcaster;

use crate::async_runtime::AsyncRuntime;
use crate::callbacks::RedundantStorageCallback;
use crate::chain_access::LipaChainAccess;
use crate::config::{Config, NodeAddress};
use crate::errors::{InitializationError, RuntimeError};
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::keys_manager::{generate_random_bytes, generate_secret, init_keys_manager};
use crate::logger::LightningLogger;
use crate::native_logger::init_native_logger_once;
use crate::secret::Secret;
use crate::storage_persister::StoragePersister;
use crate::tx_broadcaster::TxBroadcaster;

use crate::p2p_networking::P2pConnections;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::Network;
use esplora_client::blocking::BlockingClient as EsploraClient;
use esplora_client::Builder;
use lightning::chain::chainmonitor::ChainMonitor as LdkChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Recipient};
use lightning::chain::{BestBlock, Watch};
use lightning::ln::channelmanager::{ChainParameters, SimpleArcChannelManager};
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::scoring::{ProbabilisticScorer, ProbabilisticScoringParameters};
use lightning::util::config::UserConfig;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;
use log::{warn, Level as LogLevel};
use std::sync::{Arc, Mutex};

static ESPLORA_TIMEOUT_SECS: u64 = 30;

#[allow(dead_code)]
pub struct LightningNode {
    #[allow(dead_code)]
    rt: AsyncRuntime,
    esplora_client: Arc<EsploraClient>,
    background_processor: BackgroundProcessor,
    peer_manager: Arc<PeerManager>,
}

type ChainMonitor = LdkChainMonitor<
    InMemorySigner,
    Arc<LipaChainAccess>,
    Arc<TxBroadcaster>,
    Arc<FeeEstimator>,
    Arc<LightningLogger>,
    Arc<StoragePersister>,
>;

type ChannelManager =
    SimpleArcChannelManager<ChainMonitor, TxBroadcaster, FeeEstimator, LightningLogger>;

pub(crate) type PeerManager = lightning::ln::peer_handler::PeerManager<
    SocketDescriptor,
    Arc<ChannelManager>,
    IgnoringMessageHandler,
    Arc<LightningLogger>,
    IgnoringMessageHandler,
>;

impl LightningNode {
    pub fn new(
        config: &Config,
        redundant_storage_callback: Box<dyn RedundantStorageCallback>,
    ) -> Result<Self, InitializationError> {
        let rt = AsyncRuntime::new()?;

        let builder = Builder::new(&config.esplora_api_url).timeout(ESPLORA_TIMEOUT_SECS);
        let esplora_client =
            Arc::new(
                builder
                    .build_blocking()
                    .map_err(|e| InitializationError::EsploraClient {
                        message: e.to_string(),
                    })?,
            );

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
        let filter = Arc::new(LipaChainAccess::new(Arc::clone(&esplora_client)));

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
        if let Some(_channel_manager_block_hash) = channel_manager_block_hash {
            // TODO: You MUST rescan any blocks along the “reorg path”
            // (ie call block_disconnected() until you get to a common block and
            // then call block_connected() to step towards your best block) upon
            // deserialization before using the object!
        }

        // Step 9. Sync ChannelMonitors and ChannelManager to chain tip

        // Step 10. Give ChannelMonitors to ChainMonitor
        for (_, channel_monitor) in channel_monitors {
            let funding_outpoint = channel_monitor.get_funding_txo().0;
            chain_monitor
                .watch_channel(funding_outpoint, channel_monitor)
                .map_err(|_e| InitializationError::ChainMonitorWatchChannel)?
        }

        // Step 11: Optional: Initialize the NetGraphMsgHandler
        let _graph = persister.read_graph();
        let genesis = genesis_block(config.network).header.block_hash();
        let graph = Arc::new(NetworkGraph::new(genesis, Arc::clone(&logger)));
        let rapid_gossip = Arc::new(RapidGossipSync::new(Arc::clone(&graph)));

        // Step 12. Initialize the PeerManager
        let peer_manager = Arc::new(init_peer_manager(
            Arc::clone(&channel_manager),
            &keys_manager,
            Arc::clone(&logger),
        )?);

        // Step 13. Initialize Networking
        let peer_manager_clone = Arc::clone(&peer_manager);

        rt.handle().block_on(async move {
            P2pConnections::connect_peer(&config.lsp_node, Arc::clone(&peer_manager_clone))
                .await
                .map_err(|e| InitializationError::PeerConnection {
                    message: e.to_string(),
                })
        })?;

        // Step 14. Keep LDK Up-to-date with Chain Info

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
        // more difficult but will not provide any benifits.
        let background_processor = BackgroundProcessor::start(
            persister,
            event_handler,
            chain_monitor,
            channel_manager,
            GossipSync::rapid(rapid_gossip),
            Arc::clone(&peer_manager),
            logger,
            Some(scorer),
        );

        Ok(Self {
            rt,
            esplora_client,
            background_processor,
            peer_manager,
        })
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
        our_node_secret,
        &ephemeral_bytes,
        logger,
    ))
}

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
