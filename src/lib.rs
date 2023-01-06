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
mod task_manager;
mod test_utils;
mod tx_broadcaster;
mod types;

use crate::async_runtime::{AsyncRuntime, RepeatingTaskHandle};
use crate::callbacks::{EventsCallback, LspCallback, RemoteStorageCallback};
use crate::chain_access::LipaChainAccess;
use crate::config::Config;
use crate::confirm::ConfirmWrapper;
use crate::errors::*;
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::filter::FilterImpl;
use crate::invoice::create_raw_invoice;
pub use crate::invoice::InvoiceDetails;
use crate::keys_manager::{
    generate_random_bytes, generate_secret, init_keys_manager, mnemonic_to_secret,
};
use crate::logger::LightningLogger;
use crate::lsp::{LspClient, LspFee};
use crate::native_logger::init_native_logger_once;
use crate::node_info::{get_channels_info, ChannelsInfo, NodeInfo};
use crate::secret::Secret;
use crate::storage_persister::StoragePersister;
use crate::task_manager::{TaskManager, TaskPeriods};
use crate::tx_broadcaster::TxBroadcaster;
use crate::types::{ChainMonitor, ChannelManager, InvoicePayer, PeerManager, RapidGossipSync};
use std::fmt::Debug;
use std::str::FromStr;

use bitcoin::bech32::ToBase32;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::secp256k1::ecdsa::RecoverableSignature;
use bitcoin::Network;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::{InMemorySigner, KeysInterface, KeysManager, Recipient};
use lightning::chain::{BestBlock, ChannelMonitorUpdateStatus, Watch};
use lightning::ln::channelmanager::ChainParameters;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::router::DefaultRouter;
use lightning::util::config::UserConfig;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_invoice::payment::{PaymentError, Retry};
use lightning_invoice::{Currency, Invoice, InvoiceDescription};
use lightning_rapid_gossip_sync::GraphSyncError;
use log::{debug, error, info, warn, Level as LogLevel};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, Instant};

const FOREGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_lsp_info: Some(Duration::from_secs(10 * 60)),
    reconnect_to_lsp: Duration::from_secs(10),
    update_fees: Some(Duration::from_secs(5 * 60)),
};

const BACKGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_lsp_info: None,
    reconnect_to_lsp: Duration::from_secs(60),
    update_fees: None,
};

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
    sync_handle: RepeatingTaskHandle,
    rgs_url: String,
    rapid_sync: Arc<RapidGossipSync>,
    invoice_payer: Arc<InvoicePayer>,
    task_manager: Arc<Mutex<TaskManager>>,
}

impl LightningNode {
    #[allow(clippy::result_large_err)]
    pub fn new(
        config: &Config,
        remote_storage_callback: Box<dyn RemoteStorageCallback>,
        lsp_callback: Box<dyn LspCallback>,
        events_callback: Box<dyn EventsCallback>,
    ) -> LipaResult<Self> {
        let rt = AsyncRuntime::new()?;
        let genesis_hash = genesis_block(config.network).header.block_hash();

        let esplora_client = Arc::new(EsploraClient::new(&config.esplora_api_url.clone())?);

        // Step 1. Initialize the FeeEstimator
        let fee_estimator = Arc::new(FeeEstimator::new(
            Arc::clone(&esplora_client),
            config.network,
        ));

        // Step 2. Initialize the Logger
        let logger = Arc::new(LightningLogger {});

        // Step 3. Initialize the BroadcasterInterface
        let tx_broadcaster = Arc::new(TxBroadcaster::new(Arc::clone(&esplora_client)));

        // Step 4. Initialize Persist
        let persister = Arc::new(StoragePersister::new(
            remote_storage_callback,
            &config.local_persistence_path,
        ));
        if !persister.check_health() {
            warn!("Remote storage is unhealty");
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
        let keys_manager = Arc::new(init_keys_manager(&config.seed)?);

        // Step 7. Read ChannelMonitor state from disk
        let mut channel_monitors = persister.read_channel_monitors(&*keys_manager)?;

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
                    return Err(permanent_failure(
                        "Failed to give a ChannelMonitor to the ChainMonitor",
                    ))
                }
                ChannelMonitorUpdateStatus::PermanentFailure => {
                    return Err(permanent_failure(
                        "Failed to give a ChannelMonitor to the ChainMonitor",
                    ))
                }
            }
        }

        // Step 11: Optional: Initialize rapid sync
        let graph = Arc::new(persister.read_or_init_graph(genesis_hash, Arc::clone(&logger))?);
        let rapid_sync = Arc::new(RapidGossipSync::new(Arc::clone(&graph)));

        // Step 12. Initialize the PeerManager
        let peer_manager = Arc::new(init_peer_manager(
            Arc::clone(&channel_manager),
            &keys_manager,
            Arc::clone(&logger),
        )?);

        // Step 13. Initialize Networking
        // Skip it, since the node does not listen to incoming connections.

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

        let lsp_client = Arc::new(LspClient::new(lsp_callback));

        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            Arc::clone(&lsp_client),
            Arc::clone(&peer_manager),
            Arc::clone(&fee_estimator),
        )));
        task_manager.lock().unwrap().restart(FOREGROUND_PERIODS);

        // Step 15. Initialize an EventHandler
        let event_handler = Arc::new(LipaEventHandler::new(
            Arc::clone(&channel_manager),
            Arc::clone(&task_manager),
            events_callback,
        ));

        // Step 16. Initialize the ProbabilisticScorer
        let scorer = Arc::new(Mutex::new(
            persister.read_or_init_scorer(Arc::clone(&graph), Arc::clone(&logger))?,
        ));

        // Step 17. Initialize the InvoicePayer
        let router = DefaultRouter::new(
            graph,
            Arc::clone(&logger),
            keys_manager.get_secure_random_bytes(),
            Arc::clone(&scorer),
        );
        let invoice_payer = Arc::new(InvoicePayer::new(
            Arc::clone(&channel_manager),
            router,
            Arc::clone(&logger),
            Arc::clone(&event_handler),
            Retry::Timeout(Duration::from_secs(10)),
        ));

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
            GossipSync::rapid(Arc::clone(&rapid_sync)),
            Arc::clone(&peer_manager),
            logger,
            Some(scorer),
        );

        Ok(Self {
            network: config.network,
            rt,
            esplora_client,
            lsp_client,
            keys_manager,
            background_processor,
            channel_manager: Arc::clone(&channel_manager),
            peer_manager,
            sync_handle,
            rgs_url: config.rgs_url.clone(),
            rapid_sync,
            invoice_payer,
            task_manager,
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
            .task_manager
            .lock()
            .unwrap()
            .get_lsp_info()
            .ok_or_runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to get LSP info",
            )?;
        Ok(lsp_info.fee)
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

    // Not exposed in UDL. For intergration tests and example node use only.
    pub fn sync_graph(&self) -> LipaResult<()> {
        let last_sync_timestamp = self
            .rapid_sync
            .network_graph()
            .get_last_rapid_gossip_sync_timestamp()
            .unwrap_or(0);

        let snapshot_contents =
            reqwest::blocking::get(format!("{}{}", self.rgs_url, last_sync_timestamp))
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

    pub fn decode_invoice(&self, invoice: String) -> LipaResult<InvoiceDetails> {
        let invoice = Self::parse_validate_invoice(self, &invoice)?;

        let description = match invoice.description() {
            InvoiceDescription::Direct(d) => d.to_string(),
            InvoiceDescription::Hash(_) => String::new(),
        };

        let payee_pub_key = match invoice.payee_pub_key() {
            None => invoice.recover_payee_pub_key().to_string(),
            Some(p) => p.to_string(),
        };

        Ok(InvoiceDetails {
            amount_msat: invoice.amount_milli_satoshis(),
            description,
            payment_hash: invoice.payment_hash().to_string(),
            payee_pub_key,
            invoice_timestamp: invoice.timestamp(),
            expiry_interval: invoice.expiry_time(),
        })
    }

    pub fn pay_invoice(&self, invoice: String) -> LipaResult<()> {
        let invoice = Self::parse_validate_invoice(self, &invoice)?;

        let amount_msat = invoice
	    .amount_milli_satoshis()
	    .ok_or_invalid_input("Invalid invoice - invoice is a zero value invoice and paying such invoice is not supported yet")?;

        match self.invoice_payer.pay_invoice(&invoice) {
            Ok(_payment_id) => {
                info!("Initiated payment of {} msats", amount_msat);
            }
            Err(e) => {
                return match e {
                    PaymentError::Invoice(e) => {
                        Err(invalid_input(format!("Invalid invoice - {}", e)))
                    }
                    PaymentError::Routing(e) => Err(runtime_error(
                        RuntimeErrorCode::NoRouteFound,
                        format!(
                            "Failed to find a route - {} - Recommended action: {:?}",
                            e.err, e.action
                        ),
                    )),
                    PaymentError::Sending(e) => Err(runtime_error(
                        RuntimeErrorCode::SendFailure,
                        format!("Failed to send payment - {:?}", e),
                    )),
                }
            }
        }
        Ok(())
    }

    pub fn foreground(&self) {
        self.task_manager
            .lock()
            .unwrap()
            .restart(FOREGROUND_PERIODS);
    }

    pub fn background(&self) {
        self.task_manager
            .lock()
            .unwrap()
            .restart(BACKGROUND_PERIODS);
    }

    fn parse_validate_invoice(&self, invoice: &str) -> LipaResult<Invoice> {
        let invoice = Invoice::from_str(Self::chomp_prefix(invoice.trim()))
            .map_to_invalid_input("Invalid invoice - parse failure")?;

        let network = match invoice.currency() {
            Currency::Bitcoin => Network::Bitcoin,
            Currency::BitcoinTestnet => Network::Testnet,
            Currency::Regtest => Network::Regtest,
            Currency::Simnet => Network::Signet,
            Currency::Signet => Network::Signet,
        };

        if network != self.network {
            return Err(invalid_input("Invalid invoice - network mismatch"));
        }

        Ok(invoice)
    }

    fn chomp_prefix(string: &str) -> &str {
        let prefix = "lightning:";
        if let Some(tail) = string.strip_prefix(prefix) {
            tail
        } else {
            string
        }
    }
}

impl Drop for LightningNode {
    fn drop(&mut self) {
        self.sync_handle.blocking_shutdown();
        self.task_manager.lock().unwrap().request_shutdown_all();

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

    // Do not limit inbound HTLC size (default is to allow only 10% of channel size)
    user_config
        .channel_handshake_config
        .max_inbound_htlc_value_in_flight_percent_of_channel = 100;

    // Increase the max dust htlc exposure from the 5000 sat default to 1M sats
    user_config.channel_config.max_dust_htlc_exposure_msat = 1_000_000_000;

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
) -> LipaResult<PeerManager> {
    let ephemeral_bytes = generate_random_bytes::<32>()
        .map_to_permanent_failure("Failed to generate random bytes")?;
    let our_node_secret = keys_manager
        .get_node_secret(Recipient::Node)
        .map_to_permanent_failure("Failed to get our own node secret")?;
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
