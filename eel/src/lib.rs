#![allow(clippy::let_unit_value)]

extern crate core;

pub mod config;
pub mod errors;
pub mod interfaces;
pub mod key_derivation;
pub mod keys_manager;
pub mod lsp;
pub mod node_info;
pub mod p2p_networking;
pub mod payment_store;
pub mod recovery;
pub mod secret;

mod async_runtime;
mod encryption_symmetric;
mod esplora_client;
mod event_handler;
mod fee_estimator;
mod invoice;
mod logger;
mod random;
mod rapid_sync_client;
mod storage_persister;
mod task_manager;
mod test_utils;
mod tx_broadcaster;
mod types;

use crate::async_runtime::AsyncRuntime;
use crate::config::{Config, TzConfig};
use crate::errors::*;
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::interfaces::{EventHandler, ExchangeRateProvider, ExchangeRates, RemoteStorage};
pub use crate::invoice::InvoiceDetails;
use crate::invoice::{create_invoice, validate_invoice, CreateInvoiceParams};
use crate::keys_manager::init_keys_manager;
use crate::logger::LightningLogger;
use crate::lsp::{calculate_fee, LspClient, LspFee};
use crate::node_info::{estimate_max_incoming_payment_size, get_channels_info, NodeInfo};
use crate::payment_store::{FiatValues, Payment, PaymentState, PaymentStore, PaymentType};
use crate::random::generate_random_bytes;
use crate::rapid_sync_client::RapidSyncClient;
use crate::storage_persister::StoragePersister;
use crate::task_manager::{RestartIfFailedPeriod, TaskManager, TaskPeriods};
use crate::tx_broadcaster::TxBroadcaster;
use crate::types::{
    ChainMonitor, ChannelManager, NetworkGraph, PeerManager, RapidGossipSync, Router, Scorer,
    TxSync,
};

use bitcoin::hashes::hex::{FromHex, ToHex};
pub use bitcoin::Network;
use cipher::consts::U32;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::{EntropySource, InMemorySigner, KeysManager};
use lightning::chain::{BestBlock, ChannelMonitorUpdateStatus, Confirm, Watch};
use lightning::ln::channelmanager::{ChainParameters, Retry, RetryableSendFailure};
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::util::config::UserConfig;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_invoice::payment::{pay_invoice, PaymentError};
use lightning_invoice::{Currency, Invoice, InvoiceDescription};
use log::error;
pub use log::Level as LogLevel;
use log::{info, warn};
pub use perro::{
    invalid_input, permanent_failure, runtime_error, MapToError, MapToErrorForUnitType,
    OptionToError,
};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::Duration;

const FOREGROUND_PERIODS: TaskPeriods = TaskPeriods {
    sync_blockchain: Duration::from_secs(5),
    update_lsp_info: Some(Duration::from_secs(10 * 60)),
    reconnect_to_lsp: Duration::from_secs(10),
    update_fees: Some(Duration::from_secs(5 * 60)),
    update_graph: Some(RestartIfFailedPeriod::from_secs(2 * 60)),
    update_exchange_rates: Some(Duration::from_secs(10 * 60)),
};

const BACKGROUND_PERIODS: TaskPeriods = TaskPeriods {
    sync_blockchain: Duration::from_secs(60 * 60),
    update_lsp_info: None,
    reconnect_to_lsp: Duration::from_secs(60),
    update_fees: None,
    update_graph: None,
    update_exchange_rates: None,
};

#[allow(dead_code)]
pub struct LightningNode {
    config: Config,
    keys_manager: Arc<KeysManager>,
    channel_manager: Arc<ChannelManager>,
    peer_manager: Arc<PeerManager>,
    payment_store: Arc<Mutex<PaymentStore>>,
    persister: Arc<StoragePersister>,
    chain_monitor: Arc<ChainMonitor>,
    graph: Arc<NetworkGraph>,
    logger: Arc<LightningLogger>,
    scorer: Arc<Mutex<Scorer>>,
    fee_estimator: Arc<FeeEstimator>,
    tx_sync: Arc<TxSync>,
    lsp_client: Arc<LspClient>,
    user_event_handler: Arc<Box<dyn EventHandler>>,
    exchange_rate_provider: Arc<Box<dyn ExchangeRateProvider>>,
    lightning_runtime: RwLock<Option<LightningRuntime>>,
}

struct LightningRuntime {
    pub _rt: AsyncRuntime,
    pub _background_processor: BackgroundProcessor,
    pub task_manager: Arc<Mutex<TaskManager>>,
}

impl LightningNode {
    pub fn new(
        config: Config,
        remote_storage: Box<dyn RemoteStorage>,
        user_event_handler: Box<dyn EventHandler>,
        exchange_rate_provider: Box<dyn ExchangeRateProvider>,
    ) -> Result<Self> {
        let esplora_client = Arc::new(EsploraClient::new(&config.esplora_api_url)?);

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
        ));
        if !persister.check_health() {
            warn!("Remote storage is unhealty");
        }

        // Step 5. Initialize the Transaction Filter
        let tx_sync = Arc::new(TxSync::new(
            config.esplora_api_url.clone(),
            Arc::clone(&logger),
        ));

        // Step 6. Initialize the ChainMonitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            Some(Arc::clone(&tx_sync)),
            Arc::clone(&tx_broadcaster),
            Arc::clone(&logger),
            Arc::clone(&fee_estimator),
            Arc::clone(&persister),
        ));

        // Step 7. Provide the ChainMonitor to Persist
        persister.add_chain_monitor(Arc::downgrade(&chain_monitor));

        // Step 8. Initialize the KeysManager
        let keys_manager = Arc::new(init_keys_manager(&config.get_seed_first_half())?);

        // Step 9. Read ChannelMonitor state from disk/remote
        let mut channel_monitors =
            persister.read_channel_monitors(&*keys_manager, &*keys_manager)?;

        // Step 10: Initialize the NetworkGraph
        let graph = Arc::new(persister.read_or_init_graph(config.network, Arc::clone(&logger))?);

        // Step 11: Initialize the ProbabilisticScorer
        let scorer = Arc::new(Mutex::new(
            persister.read_or_init_scorer(Arc::clone(&graph), Arc::clone(&logger))?,
        ));

        // Step 12: Initialize the Router
        let router = Arc::new(Router::new(
            Arc::clone(&graph),
            Arc::clone(&logger),
            keys_manager.get_secure_random_bytes(),
            Arc::clone(&scorer),
        ));

        // (needed when using Electrum or BIP 157/158)
        // Step 13: Prepare ChannelMonitors for chain sync
        for (_, channel_monitor) in channel_monitors.iter() {
            channel_monitor.load_outputs_to_watch(&tx_sync);
        }

        // Step 14: Initialize the ChannelManager
        let mobile_node_user_config = build_mobile_node_user_config();
        // TODO: Init properly.
        let best_block = BestBlock::from_network(config.network);
        let chain_params = ChainParameters {
            network: config.network,
            best_block,
        };
        let mut_channel_monitors: Vec<&mut ChannelMonitor<InMemorySigner>> =
            channel_monitors.iter_mut().map(|(_, m)| m).collect();

        let channel_manager = persister.read_or_init_channel_manager(
            Arc::clone(&chain_monitor),
            Arc::clone(&tx_broadcaster),
            Arc::clone(&keys_manager),
            Arc::clone(&fee_estimator),
            Arc::clone(&logger),
            Arc::clone(&router),
            mut_channel_monitors,
            mobile_node_user_config,
            chain_params,
        )?;
        let channel_manager = Arc::new(channel_manager);

        // Step 15. Sync ChannelMonitors and ChannelManager to chain tip
        let rt = AsyncRuntime::new()?;
        let confirmables = vec![
            &*channel_manager as &(dyn Confirm + Sync + Send),
            &*chain_monitor as &(dyn Confirm + Sync + Send),
        ];
        {
            let tx_sync = Arc::clone(&tx_sync);
            if let Err(e) = rt
                .handle()
                .block_on(async move { tx_sync.sync(confirmables).await })
            {
                error!("Sync to blockchain failed: {:?}", e);
            }
        }

        // Step 16. Give ChannelMonitors to ChainMonitor
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

        // Step 17. Initialize the PeerManager
        let peer_manager = Arc::new(init_peer_manager(
            Arc::clone(&channel_manager),
            Arc::clone(&keys_manager),
            Arc::clone(&logger),
        )?);

        // Step 18: Initialize the PaymentStore
        let payment_store_path = Path::new(&config.local_persistence_path).join("payment_db.db3");
        let payment_store_path = payment_store_path
            .to_str()
            .ok_or_invalid_input("Invalid local persistence path")?;
        let payment_store = Arc::new(Mutex::new(PaymentStore::new(
            payment_store_path,
            config.timezone_config.clone(),
        )?));

        let lsp_client = Arc::new(LspClient::new(
            config.lsp_url.clone(),
            config.lsp_token.clone(),
        )?);

        Ok(Self {
            config,
            keys_manager,
            channel_manager: Arc::clone(&channel_manager),
            peer_manager,
            payment_store,
            persister,
            chain_monitor,
            graph,
            logger,
            scorer,
            fee_estimator,
            tx_sync,
            lsp_client,
            user_event_handler: Arc::new(user_event_handler),
            exchange_rate_provider: Arc::new(exchange_rate_provider),
            lightning_runtime: RwLock::new(None),
        })
    }

    pub fn start(&self) -> Result<()> {
        let mut lightning_runtime = self.lightning_runtime.write().unwrap();
        if lightning_runtime.is_some() {
            return Err(runtime_error(
                RuntimeErrorCode::NodeAlreadyRunning,
                "The node is already running. Can't start it again.",
            ))?;
        }

        let rt = AsyncRuntime::new()?;

        let rapid_sync = Arc::new(RapidGossipSync::new(
            Arc::clone(&self.graph),
            Arc::clone(&self.logger),
        ));
        let rapid_sync_client = Arc::new(RapidSyncClient::new(
            self.config.rgs_url.clone(),
            Arc::clone(&rapid_sync),
        )?);

        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            Arc::clone(&self.lsp_client),
            Arc::clone(&self.peer_manager),
            Arc::clone(&self.fee_estimator),
            rapid_sync_client,
            Arc::clone(&self.channel_manager),
            Arc::clone(&self.chain_monitor),
            Arc::clone(&self.tx_sync),
            self.config.fiat_currency.clone(),
            Arc::clone(&self.exchange_rate_provider),
        )));
        task_manager.lock().unwrap().restart(FOREGROUND_PERIODS);

        let event_handler = Arc::new(LipaEventHandler::new(
            Arc::clone(&self.channel_manager),
            Arc::clone(&task_manager),
            Arc::clone(&self.user_event_handler),
            Arc::clone(&self.payment_store),
        )?);

        // The fact that we do not restart the background process assumes that
        // it will never fail. However it may fail:
        //  1. on persisting channel manager, but it never fails since we ignore
        //     such failures in StoragePersister::persist_manager()
        //  2. on persisting scorer or network graph on exit, but we do not care
        // The other strategy to handle errors and restart the process will be
        // more difficult but will not provide any benefits.
        let background_processor = BackgroundProcessor::start(
            Arc::clone(&self.persister),
            event_handler,
            Arc::clone(&self.chain_monitor),
            Arc::clone(&self.channel_manager),
            GossipSync::rapid(Arc::clone(&rapid_sync)),
            Arc::clone(&self.peer_manager),
            Arc::clone(&self.logger),
            Some(Arc::clone(&self.scorer)),
        );

        *lightning_runtime = Some(LightningRuntime {
            _rt: rt,
            _background_processor: background_processor,
            task_manager,
        });

        Ok(())
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
        let rt_lock = self.lightning_runtime.read().unwrap();
        let lightning_runtime = rt_lock
            .as_ref()
            .ok_or_runtime_error(RuntimeErrorCode::NodeNotRunning, "Node isn't running")?;

        let lsp_info = lightning_runtime
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

    pub fn calculate_lsp_fee(&self, amount_msat: u64) -> Result<u64> {
        let max_incoming_payment_size =
            estimate_max_incoming_payment_size(&self.get_node_info().channels_info);
        if max_incoming_payment_size < amount_msat {
            let lsp_fee = self.query_lsp_fee()?;
            let fee = calculate_fee(amount_msat, &lsp_fee);
            return Ok(fee);
        }
        Ok(0)
    }

    pub fn create_invoice(
        &self,
        amount_msat: u64,
        description: String,
        metadata: String,
    ) -> Result<InvoiceDetails> {
        let currency = match self.config.network {
            Network::Bitcoin => Currency::Bitcoin,
            Network::Testnet => Currency::BitcoinTestnet,
            Network::Regtest => Currency::Regtest,
            Network::Signet => Currency::Signet,
        };
        let fiat_values = self.get_fiat_values(amount_msat);
        let rt = AsyncRuntime::new()?;
        let signed_invoice = rt.handle().block_on(create_invoice(
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
            fiat_values,
        ))?;
        let invoice_str = signed_invoice.to_string();
        self.decode_invoice(invoice_str)
    }

    pub fn decode_invoice(&self, invoice: String) -> Result<InvoiceDetails> {
        let invoice = invoice::parse_invoice(&invoice)?;
        invoice::get_invoice_details(&invoice)
    }

    pub fn pay_invoice(&self, invoice: String, metadata: String) -> Result<()> {
        let invoice_struct =
            self.validate_persist_new_outgoing_payment_attempt(&invoice, &metadata)?;

        match pay_invoice(
            &invoice_struct,
            Retry::Timeout(Duration::from_secs(10)),
            &self.channel_manager,
        ) {
            Ok(_payment_id) => {
                info!(
                    "Initiated payment of {:?} msats",
                    invoice_struct.amount_milli_satoshis()
                );
            }
            Err(e) => {
                return match e {
                    PaymentError::Invoice(e) => {
                        self.payment_store.lock().unwrap().new_payment_state(
                            invoice_struct.payment_hash(),
                            PaymentState::Failed,
                        )?;
                        Err(invalid_input(format!("Invalid invoice - {e}")))
                    }
                    PaymentError::Sending(e) => {
                        self.payment_store.lock().unwrap().new_payment_state(
                            invoice_struct.payment_hash(),
                            PaymentState::Failed,
                        )?;
                        match e {
                            RetryableSendFailure::PaymentExpired => Err(runtime_error(
                                RuntimeErrorCode::SendFailure,
                                format!("Failed to send payment - {e:?}"),
                            )),
                            RetryableSendFailure::RouteNotFound => Err(runtime_error(
                                RuntimeErrorCode::NoRouteFound,
                                "Failed to find a route",
                            )),
                            RetryableSendFailure::DuplicatePayment => Err(runtime_error(
                                RuntimeErrorCode::SendFailure,
                                format!("Failed to send payment - {e:?}"),
                            )),
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_persist_new_outgoing_payment_attempt(
        &self,
        invoice: &str,
        metadata: &str,
    ) -> Result<Invoice> {
        let invoice_struct = invoice::parse_invoice(invoice)?;

        validate_invoice(self.config.network, &invoice_struct)?;

        let amount_msat = invoice_struct
            .amount_milli_satoshis()
            .ok_or_invalid_input("Invalid invoice - invoice is a zero value invoice and paying such invoice is not supported yet")?;
        let description = match invoice_struct.description() {
            InvoiceDescription::Direct(d) => d.clone().into_inner(),
            InvoiceDescription::Hash(h) => h.0.to_hex(),
        };
        let fiat_values = self.get_fiat_values(amount_msat);

        let mut payment_store = self.payment_store.lock().unwrap();
        if let Ok(payment) = payment_store.get_payment(invoice_struct.payment_hash()) {
            match payment.payment_type {
                PaymentType::Receiving => return Err(runtime_error(
                    RuntimeErrorCode::PayingToSelf,
                    "This invoice was issued by the local node. Paying yourself is not supported.",
                )),
                PaymentType::Sending => {
                    if payment.payment_state != PaymentState::Failed {
                        return Err(runtime_error(
                            RuntimeErrorCode::AlreadyUsedInvoice,
                            "This invoice has already been paid or is in the process of being paid. Please use a different one or wait until the current payment attempt fails before retrying.",
                        ));
                    }
                    payment_store
                        .new_payment_state(invoice_struct.payment_hash(), PaymentState::Retried)?;
                }
            }
        } else {
            payment_store.new_outgoing_payment(
                invoice_struct.payment_hash(),
                amount_msat,
                &description,
                invoice,
                metadata,
                fiat_values,
            )?;
        }
        Ok(invoice_struct)
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        self.payment_store
            .lock()
            .unwrap()
            .get_latest_payments(number_of_payments)
    }

    pub fn get_payment(&self, hash: &str) -> Result<Payment> {
        self.payment_store
            .lock()
            .unwrap()
            .get_payment(&Vec::from_hex(hash).map_to_invalid_input("Invalid hash")?)
    }

    pub fn foreground(&self) -> Result<()> {
        let rt_lock = self.lightning_runtime.read().unwrap();
        let lightning_runtime = rt_lock
            .as_ref()
            .ok_or_runtime_error(RuntimeErrorCode::NodeNotRunning, "Node isn't running")?;

        lightning_runtime
            .task_manager
            .lock()
            .unwrap()
            .restart(FOREGROUND_PERIODS);
        Ok(())
    }

    pub fn background(&self) -> Result<()> {
        let rt_lock = self.lightning_runtime.read().unwrap();
        let lightning_runtime = rt_lock
            .as_ref()
            .ok_or_runtime_error(RuntimeErrorCode::NodeNotRunning, "Node isn't running")?;

        lightning_runtime
            .task_manager
            .lock()
            .unwrap()
            .restart(BACKGROUND_PERIODS);
        Ok(())
    }

    pub fn get_exchange_rates(&self) -> Result<ExchangeRates> {
        let rt_lock = self.lightning_runtime.read().unwrap();
        let lightning_runtime = rt_lock
            .as_ref()
            .ok_or_runtime_error(RuntimeErrorCode::NodeNotRunning, "Node isn't running")?;

        let res = lightning_runtime
            .task_manager
            .lock()
            .unwrap()
            .get_exchange_rates()
            .ok_or_runtime_error(
                RuntimeErrorCode::ExchangeRateProviderUnavailable,
                "Failed to get exchange rates",
            );
        res
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) -> Result<()> {
        let rt_lock = self.lightning_runtime.read().unwrap();
        let lightning_runtime = rt_lock
            .as_ref()
            .ok_or_runtime_error(RuntimeErrorCode::NodeNotRunning, "Node isn't running")?;

        let mut task_manager = lightning_runtime.task_manager.lock().unwrap();
        task_manager.change_fiat_currency(fiat_currency);
        // if the fiat currency is being changed, we can assume the app is in the foreground
        task_manager.restart(FOREGROUND_PERIODS);
        Ok(())
    }

    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        let mut payment_store = self.payment_store.lock().unwrap();
        payment_store.update_timezone_config(timezone_config);
    }

    fn get_fiat_values(&self, amount_msat: u64) -> Option<FiatValues> {
        self.get_exchange_rates()
            .ok()
            .map(|e| FiatValues::from_amount_msat(amount_msat, &e))
    }

    pub fn stop(&self) -> Result<()> {
        let mut rt_lock = self.lightning_runtime.write().unwrap();
        let lightning_runtime = rt_lock
            .as_ref()
            .ok_or_runtime_error(RuntimeErrorCode::NodeNotRunning, "Node isn't running")?;

        lightning_runtime
            .task_manager
            .lock()
            .unwrap()
            .request_shutdown_all();

        self.peer_manager.disconnect_all_peers();

        // Dropping the LightningRuntime stops the background processor
        *rt_lock = None;
        Ok(())
    }
}

impl Drop for LightningNode {
    fn drop(&mut self) {
        let _ = self.stop();
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
    keys_manager: Arc<KeysManager>,
    logger: Arc<LightningLogger>,
) -> Result<PeerManager> {
    let ephemeral_bytes = generate_random_bytes::<U32>()
        .map_to_permanent_failure("Failed to generate random bytes")?;
    Ok(PeerManager::new_channel_only(
        channel_manager,
        IgnoringMessageHandler {},
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32,
        ephemeral_bytes.as_ref(),
        logger,
        keys_manager,
    ))
}
