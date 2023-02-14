#![allow(clippy::let_unit_value)]

extern crate core;

pub mod config;
pub mod errors;
pub mod interfaces;
pub mod keys_manager;
pub mod lsp;
pub mod node_info;
pub mod p2p_networking;
pub mod payment_store;
pub mod secret;

mod async_runtime;
mod chain_access;
mod confirm;
mod encryption_symmetric;
mod esplora_client;
mod event_handler;
mod fee_estimator;
mod filter;
mod invoice;
mod key_derivation;
mod logger;
mod random;
mod rapid_sync_client;
mod storage_persister;
mod task_manager;
mod test_utils;
mod tx_broadcaster;
mod types;

use crate::async_runtime::AsyncRuntime;
use crate::chain_access::LipaChainAccess;
use crate::config::Config;
use crate::confirm::ConfirmWrapper;
use crate::errors::*;
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::filter::FilterImpl;
use crate::interfaces::{EventHandler, RemoteStorage};
pub use crate::invoice::InvoiceDetails;
use crate::invoice::{create_invoice, CreateInvoiceParams};
use crate::keys_manager::init_keys_manager;
use crate::logger::LightningLogger;
use crate::lsp::{LspClient, LspFee};
use crate::node_info::{get_channels_info, NodeInfo};
use crate::random::generate_random_bytes;
use crate::rapid_sync_client::RapidSyncClient;
use crate::storage_persister::StoragePersister;
use crate::task_manager::{RestartIfFailedPeriod, TaskManager, TaskPeriods};
use crate::tx_broadcaster::TxBroadcaster;
use crate::types::{ChainMonitor, ChannelManager, InvoicePayer, PeerManager, RapidGossipSync};
use std::path::Path;

use crate::payment_store::{Payment, PaymentStore};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::hashes::hex::ToHex;
pub use bitcoin::Network;
use cipher::consts::U32;
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
pub use log::Level as LogLevel;
use log::{info, warn};
pub use perro::{
    invalid_input, permanent_failure, runtime_error, MapToError, MapToErrorForUnitType,
    OptionToError,
};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::Duration;

const FOREGROUND_PERIODS: TaskPeriods = TaskPeriods {
    sync_blockchain: Duration::from_secs(5),
    update_lsp_info: Some(Duration::from_secs(10 * 60)),
    reconnect_to_lsp: Duration::from_secs(10),
    update_fees: Some(Duration::from_secs(5 * 60)),
    update_graph: Some(RestartIfFailedPeriod::from_secs(2 * 60)),
};

const BACKGROUND_PERIODS: TaskPeriods = TaskPeriods {
    sync_blockchain: Duration::from_secs(60 * 60),
    update_lsp_info: None,
    reconnect_to_lsp: Duration::from_secs(60),
    update_fees: None,
    update_graph: None,
};

#[allow(dead_code)]
pub struct LightningNode {
    network: Network,
    rt: AsyncRuntime,
    lsp_client: Arc<LspClient>,
    keys_manager: Arc<KeysManager>,
    background_processor: BackgroundProcessor,
    channel_manager: Arc<ChannelManager>,
    peer_manager: Arc<PeerManager>,
    invoice_payer: Arc<InvoicePayer>,
    task_manager: Arc<Mutex<TaskManager>>,
    payment_store: Mutex<PaymentStore>,
}

#[derive(Debug, PartialEq, Eq)]
enum StartupVariant {
    FreshStart,
    Recovery,
    Normal,
}

impl LightningNode {
    pub fn new(
        config: &Config,
        remote_storage: Box<dyn RemoteStorage>,
        user_event_handler: Box<dyn EventHandler>,
    ) -> Result<Self> {
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
        let encryption_key =
            key_derivation::derive_persistence_encryption_key(&config.seed).unwrap();
        let persister = Arc::new(StoragePersister::new(
            remote_storage,
            config.local_persistence_path.clone(),
            encryption_key,
            rt.handle(),
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

        persister.add_chain_monitor(Arc::downgrade(&chain_monitor));

        // Step 6. Initialize the KeysManager
        let keys_manager = Arc::new(init_keys_manager(&config.get_seed_first_half())?);

        // Step 7. Read ChannelMonitor state from disk/remote
        let (startup_variant, mut channel_monitors) =
            persister.read_channel_monitors(&*keys_manager)?;

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
                startup_variant,
            )?;
        let channel_manager = Arc::new(channel_manager);

        // Step 9. Sync ChannelMonitors and ChannelManager to chain tip
        let confirm = ConfirmWrapper::new(vec![&*channel_manager, &*chain_monitor]);
        let chain_access = Arc::new(Mutex::new(LipaChainAccess::new(
            esplora_client,
            filter,
            channel_manager_block_hash.unwrap_or(genesis_hash),
        )));
        chain_access.lock().unwrap().sync(&confirm)?;

        // Step 10. Give ChannelMonitors to ChainMonitor
        for (_, channel_monitor) in channel_monitors {
            let funding_outpoint = channel_monitor.get_funding_txo().0;
            match chain_monitor.watch_channel(funding_outpoint, channel_monitor) {
                ChannelMonitorUpdateStatus::Completed => {}
                ChannelMonitorUpdateStatus::InProgress => {}
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
        let rapid_sync_client = Arc::new(RapidSyncClient::new(
            config.rgs_url.clone(),
            Arc::clone(&rapid_sync),
        )?);

        // Step 12. Initialize the PeerManager
        let peer_manager = Arc::new(init_peer_manager(
            Arc::clone(&channel_manager),
            &keys_manager,
            Arc::clone(&logger),
        )?);

        // Step 13. Initialize Networking
        // Skip it, since the node does not listen to incoming connections.

        // Step 14. Keep LDK Up-to-date with Chain Info
        // Implemented in TaskManager.

        let lsp_client = Arc::new(LspClient::new(
            config.lsp_url.clone(),
            config.lsp_token.clone(),
        )?);

        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            Arc::clone(&lsp_client),
            Arc::clone(&peer_manager),
            Arc::clone(&fee_estimator),
            rapid_sync_client,
            Arc::clone(&channel_manager),
            Arc::clone(&chain_monitor),
            Arc::clone(&chain_access),
        )));
        task_manager.lock().unwrap().restart(FOREGROUND_PERIODS);

        let payment_store_path = Path::new(&config.local_persistence_path).join("payment_db.db3");
        let payment_store_path = payment_store_path
            .to_str()
            .ok_or_invalid_input("Invalid local persistence path")?;

        let payment_store = Mutex::new(PaymentStore::new(payment_store_path)?);

        // Step 15. Initialize an EventHandler
        let event_handler = Arc::new(LipaEventHandler::new(
            Arc::clone(&channel_manager),
            Arc::clone(&task_manager),
            user_event_handler,
            payment_store_path,
        )?);

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
            GossipSync::rapid(rapid_sync),
            Arc::clone(&peer_manager),
            logger,
            Some(scorer),
        );

        Ok(Self {
            network: config.network,
            rt,
            lsp_client,
            keys_manager,
            background_processor,
            channel_manager: Arc::clone(&channel_manager),
            peer_manager,
            invoice_payer,
            task_manager,
            payment_store,
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

    pub fn query_lsp_fee(&self) -> Result<LspFee> {
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

    pub fn create_invoice(
        &self,
        amount_msat: u64,
        description: String,
        metadata: Vec<u8>,
    ) -> Result<String> {
        let currency = match self.network {
            Network::Bitcoin => Currency::Bitcoin,
            Network::Testnet => Currency::BitcoinTestnet,
            Network::Regtest => Currency::Regtest,
            Network::Signet => Currency::Signet,
        };
        let signed_invoice = self.rt.handle().block_on(create_invoice(
            CreateInvoiceParams {
                amount_msat,
                currency,
                description,
                metadata,
            },
            &self.channel_manager,
            &self.lsp_client,
            &self.keys_manager,
            &mut self.payment_store.lock().unwrap(),
        ))?;
        Ok(signed_invoice.to_string())
    }

    pub fn decode_invoice(&self, invoice: String) -> Result<InvoiceDetails> {
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

    pub fn pay_invoice(&self, invoice: String, metadata: Vec<u8>) -> Result<()> {
        let invoice_struct = Self::parse_validate_invoice(self, &invoice)?;

        let amount_msat = invoice_struct
	    .amount_milli_satoshis()
	    .ok_or_invalid_input("Invalid invoice - invoice is a zero value invoice and paying such invoice is not supported yet")?;

        let description = match invoice_struct.description() {
            InvoiceDescription::Direct(d) => d.clone().into_inner(),
            InvoiceDescription::Hash(h) => h.0.to_hex(),
        };

        match self.invoice_payer.pay_invoice(&invoice_struct) {
            Ok(_payment_id) => {
                info!("Initiated payment of {} msats", amount_msat);
                self.payment_store.lock().unwrap().new_outgoing_payment(
                    invoice_struct.payment_hash(),
                    amount_msat,
                    &description,
                    &invoice,
                    &metadata,
                )?;
            }
            Err(e) => match e {
                PaymentError::Invoice(e) => {
                    return Err(invalid_input(format!("Invalid invoice - {e}")))
                }
                PaymentError::Routing(e) => {
                    let mut payment_store = self.payment_store.lock().unwrap();
                    payment_store.new_outgoing_payment(
                        invoice_struct.payment_hash(),
                        amount_msat,
                        &description,
                        &invoice,
                        &metadata,
                    )?;
                    payment_store.payment_failed(invoice_struct.payment_hash())?;
                    return Err(runtime_error(
                        RuntimeErrorCode::NoRouteFound,
                        format!(
                            "Failed to find a route - {} - Recommended action: {:?}",
                            e.err, e.action
                        ),
                    ));
                }
                PaymentError::Sending(e) => {
                    let mut payment_store = self.payment_store.lock().unwrap();
                    payment_store.new_outgoing_payment(
                        invoice_struct.payment_hash(),
                        amount_msat,
                        &description,
                        &invoice,
                        &metadata,
                    )?;
                    payment_store.payment_failed(invoice_struct.payment_hash())?;
                    return Err(runtime_error(
                        RuntimeErrorCode::SendFailure,
                        format!("Failed to send payment - {e:?}"),
                    ));
                }
            },
        }
        Ok(())
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        self.payment_store
            .lock()
            .unwrap()
            .get_latest_payments(number_of_payments)
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

    fn parse_validate_invoice(&self, invoice: &str) -> Result<Invoice> {
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
        self.task_manager.lock().unwrap().request_shutdown_all();

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

fn init_peer_manager(
    channel_manager: Arc<ChannelManager>,
    keys_manager: &KeysManager,
    logger: Arc<LightningLogger>,
) -> Result<PeerManager> {
    let ephemeral_bytes = generate_random_bytes::<U32>()
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
        ephemeral_bytes.as_ref(),
        logger,
    ))
}
