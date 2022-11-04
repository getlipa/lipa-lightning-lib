#![allow(clippy::let_unit_value)]

extern crate core;

pub mod callbacks;
pub mod config;
pub mod errors;
pub mod keys_manager;
pub mod node_info;
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
mod invoice;
mod logger;
mod lsp;
mod native_logger;
mod storage_persister;
mod test_utils;
mod tx_broadcaster;
mod types;

use crate::async_runtime::{AsyncRuntime, RepeatingTaskHandle};
use crate::callbacks::{LspCallback, RedundantStorageCallback};
use crate::chain_access::LipaChainAccess;
use crate::config::{Config, NodeAddress};
use crate::confirm::ConfirmWrapper;
use crate::errors::*;
use crate::errors::{InitializationError, LipaError, LspError, RuntimeError};
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::filter::FilterImpl;
use crate::invoice::create_raw_invoice;
use crate::keys_manager::{
    generate_random_bytes, generate_secret, init_keys_manager, mnemonic_to_secret,
};
use crate::logger::LightningLogger;
use crate::lsp::LspClient;
use crate::lsp::LspFee;
use crate::native_logger::init_native_logger_once;
use crate::node_info::{get_channels_info, ChannelsInfo, NodeInfo};
use crate::p2p_networking::{LnPeer, P2pConnection};
use crate::secret::Secret;
use crate::storage_persister::StoragePersister;
use crate::tx_broadcaster::TxBroadcaster;
use crate::types::{ChainMonitor, ChannelManager, PeerManager};

use bitcoin::bech32::ToBase32;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::hashes::hex::FromHex;
use bitcoin::secp256k1::ecdsa::RecoverableSignature;
use bitcoin::secp256k1::PublicKey;
use bitcoin::Network;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Recipient};
use lightning::chain::{BestBlock, ChannelMonitorUpdateStatus, Watch};
use lightning::ln::channelmanager::ChainParameters;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::scoring::{ProbabilisticScorer, ProbabilisticScoringParameters};
use lightning::util::config::UserConfig;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_invoice::Currency;
use lightning_rapid_gossip_sync::RapidGossipSync;
use log::{debug, error, info, warn, Level as LogLevel};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, Instant};

#[allow(dead_code)]
pub struct LightningNode {
    network: Network,
    rt: AsyncRuntime,
    esplora_client: Arc<EsploraClient>,
    lsp_client: Arc<LspClient>,
    keys_manager: Arc<KeysManager>,
    background_processor: BackgroundProcessor,
    channel_manager: Arc<ChannelManager>,
    peer_manager: Arc<PeerManager>,
    p2p_connector_handle: RepeatingTaskHandle,
    sync_handle: RepeatingTaskHandle,
}

impl LightningNode {
    #[allow(clippy::result_large_err)]
    pub fn new(
        config: &Config,
        redundant_storage_callback: Box<dyn RedundantStorageCallback>,
        lsp_callback: Box<dyn LspCallback>,
    ) -> Result<Self, InitializationError> {
        let rt = AsyncRuntime::new()?;
        let genesis_hash = genesis_block(config.network).header.block_hash();

        let esplora_client = Arc::new(
            EsploraClient::new(&config.esplora_api_url.clone()).map_err(|e| {
                InitializationError::EsploraClient {
                    message: e.to_string(),
                }
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
        chain_access.lock().unwrap().sync(&confirm).map_err(|e| {
            InitializationError::EsploraClient {
                message: e.to_string(),
            }
        })?;

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

        // Step 11: Optional: Initialize rapid sync
        let graph = Arc::new(
            persister
                .read_or_init_graph(genesis_hash, Arc::clone(&logger))
                .unwrap(),
        );
        let rapid_sync = Arc::new(RapidGossipSync::new(Arc::clone(&graph)));
        let last_sync_timestamp = graph.get_last_rapid_gossip_sync_timestamp().unwrap_or(0);
        info!(
            "Starting rapid sync from timestamp: {}",
            last_sync_timestamp
        );

        let snapshot_contents = reqwest::blocking::get(format!(
            "{}{}",
            "https://rapidsync.lightningdevkit.org/snapshot/", last_sync_timestamp
        ))
        .unwrap()
        .bytes()
        .unwrap()
        .to_vec();

        let new_last_sync_timestamp = match rapid_sync.update_network_graph(&snapshot_contents) {
            Ok(timestamp) => timestamp,
            Err(e) => {
                error!("Error updating network graph: {:?}", e);
                last_sync_timestamp
            }
        };
        info!(
            "Finished rapid gossip sync up to timestamp: {}",
            new_last_sync_timestamp
        );

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
        let lsp_pubkey =
            PublicKey::from_slice(&Vec::from_hex(&config.lsp_node.pub_key).map_err(|e| {
                InitializationError::PublicKey {
                    message: e.to_string(),
                }
            })?)
            .map_err(|e| InitializationError::PublicKey {
                message: e.to_string(),
            })?;
        let event_handler = Arc::new(LipaEventHandler::new(
            lsp_pubkey,
            Arc::clone(&channel_manager),
        ));

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
            GossipSync::rapid(rapid_sync),
            Arc::clone(&peer_manager),
            logger,
            Some(scorer),
        );

        let lsp_client = Arc::new(LspClient::new(lsp_callback));

        Ok(Self {
            network: config.network,
            rt,
            esplora_client,
            lsp_client,
            keys_manager,
            background_processor,
            channel_manager: Arc::clone(&channel_manager),
            peer_manager,
            p2p_connector_handle,
            sync_handle,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        let channels_info = get_channels_info(&self.channel_manager.list_channels());
        NodeInfo {
            node_pubkey: self.channel_manager.get_our_node_id().serialize().to_vec(),
            num_peers: self.peer_manager.get_peer_node_ids().len() as u16,
            channels_info,
        }
    }

    pub fn query_lsp_fee(&self) -> LipaResult<LspFee> {
        let lsp_info = self
            .lsp_client
            .query_info()
            .lift_invalid_input()
            .prefix_error("Failed to query LSPD")?;
        Ok(lsp_info.fee)
    }

    pub fn connected_to_node(&self, lsp_node: &NodeAddress) -> bool {
        let peer = Arc::new(LnPeer::try_from(lsp_node).unwrap()); // todo proper error handling instead of unwrap()
        self.peer_manager
            .get_peer_node_ids()
            .contains(&peer.pub_key)
    }

    pub fn create_invoice(&self, amount_msat: u64, description: String) -> LipaResult<String> {
        let currency = match self.network {
            Network::Bitcoin => Currency::Bitcoin,
            Network::Testnet => Currency::BitcoinTestnet,
            Network::Regtest => Currency::Regtest,
            Network::Signet => Currency::Signet,
        };
        let raw_invoice = create_raw_invoice(
            amount_msat,
            currency,
            description,
            &self.channel_manager,
            &self.lsp_client,
        )?;
        let signature = self
            .keys_manager
            .sign_invoice(
                raw_invoice.hrp.to_string().as_bytes(),
                &raw_invoice.data.to_base32(),
                Recipient::Node,
            )
            .map_to_permanent_failure("Failed to sign invoice")?;
        let signed_invoice = raw_invoice
            .sign(|_| Ok::<RecoverableSignature, ()>(signature))
            .map_to_permanent_failure("Failed to sign invoice")?;
        Ok(signed_invoice.to_string())
    }
}

impl Drop for LightningNode {
    fn drop(&mut self) {
        self.p2p_connector_handle.blocking_shutdown();
        self.sync_handle.blocking_shutdown();

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

    // Manually accept inbound requests to open a new channel to support
    // zero-conf channels.
    user_config.manually_accept_inbound_channels = true;

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
    let ephemeral_bytes =
        generate_random_bytes::<32>().map_err(|e| InitializationError::PeerConnection {
            message: e.to_string(),
        })?;
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

include!(concat!(env!("OUT_DIR"), "/lipalightninglib.uniffi.rs"));
