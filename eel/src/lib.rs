#![allow(clippy::let_unit_value)]

extern crate core;

pub mod config;
mod data_store;
pub mod errors;
pub mod interfaces;
pub mod key_derivation;
pub mod keys_manager;
pub mod limits;
pub mod lsp;
mod migrations;
pub mod node_info;
pub mod p2p_networking;
pub mod payment;
pub mod recovery;
mod schema_migration;
pub mod secret;

mod async_runtime;
mod encryption_symmetric;
mod esplora_client;
mod event_handler;
mod fee_estimator;
pub mod invoice;
mod logger;
mod random;
mod rapid_sync_client;
mod router;
mod storage_persister;
mod task_manager;
mod test_utils;
mod tx_broadcaster;
mod types;

use crate::async_runtime::AsyncRuntime;
use crate::config::{Config, TzConfig};
use crate::data_store::DataStore;
use crate::errors::*;
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::interfaces::{EventHandler, ExchangeRate, ExchangeRateProvider, RemoteStorage};
use crate::invoice::{create_invoice, CreateInvoiceParams};
use crate::keys_manager::init_keys_manager;
use crate::limits::PaymentAmountLimits;
use crate::logger::LightningLogger;
use crate::lsp::{calculate_fee, LspClient, LspFee};
use crate::node_info::{estimate_max_incoming_payment_size, get_channels_info, NodeInfo};
use crate::payment::{Payment, PaymentState, PaymentType};
use crate::random::generate_random_bytes;
use crate::rapid_sync_client::RapidSyncClient;
use crate::storage_persister::StoragePersister;
use crate::task_manager::{PeriodConfig, RestartIfFailedPeriod, TaskManager, TaskPeriods};
use crate::tx_broadcaster::TxBroadcaster;
use crate::types::{ChainMonitor, ChannelManager, PeerManager, RapidGossipSync, Router, TxSync};

pub use crate::router::MaxRoutingFeeMode;
use crate::router::{FeeLimitingRouter, SimpleMaxRoutingFeeStrategy};
use bitcoin::hashes::hex::ToHex;
pub use bitcoin::Network;
use cipher::consts::U32;
use invoice::DecodeInvoiceError;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::{BestBlock, ChannelMonitorUpdateStatus, Confirm, Watch};
use lightning::ln::channelmanager::{ChainParameters, Retry, RetryableSendFailure};
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::routing::scoring::ProbabilisticScoringFeeParameters;
use lightning::sign::{EntropySource, InMemorySigner, KeysManager, SpendableOutputDescriptor};
use lightning::util::config::{MaxDustHTLCExposure, UserConfig};
use lightning::util::message_signing::sign;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_invoice::payment::{pay_invoice, pay_zero_value_invoice, PaymentError};
use lightning_invoice::Currency;
pub use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescription};
use lnurl::{LnUrlResponse, Response};
pub use log::Level as LogLevel;
use log::{error, trace};
use log::{info, warn};
pub use perro::{
    invalid_input, permanent_failure, runtime_error, MapToError, MapToErrorForUnitType,
    OptionToError,
};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::Duration;

const PAYMENT_TIMEOUT: Duration = Duration::from_secs(90);

const FOREGROUND_PERIODS: TaskPeriods = TaskPeriods {
    sync_blockchain: Duration::from_secs(5 * 60),
    update_lsp_info: Some(PeriodConfig {
        success_period: Duration::from_secs(10 * 60),
        failure_period: Duration::from_secs(5),
    }),
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
    config: Mutex<Config>,
    rt: AsyncRuntime,
    lsp_client: Arc<LspClient>,
    keys_manager: Arc<KeysManager>,
    background_processor: BackgroundProcessor,
    channel_manager: Arc<ChannelManager>,
    peer_manager: Arc<PeerManager>,
    task_manager: Arc<Mutex<TaskManager>>,
    data_store: Arc<Mutex<DataStore>>,
    max_routing_fee_strategy: Arc<SimpleMaxRoutingFeeStrategy>,
}

impl LightningNode {
    pub fn new(
        config: Config,
        remote_storage: Box<dyn RemoteStorage>,
        user_event_handler: Box<dyn EventHandler>,
        exchange_rate_provider: Box<dyn ExchangeRateProvider>,
    ) -> Result<Self> {
        trace!(
            "Creating a new LightningNode instance with:\n\
            network: {}\n\
            fiat_currency: {}\n\
            esplora_api_url: {}\n\
            lsp_url: {}\n\
            rgs_url: {}\n\
            local_persistence_path: {}",
            config.network,
            config.fiat_currency,
            config.esplora_api_url,
            config.lsp_url,
            config.rgs_url,
            config.local_persistence_path,
        );

        let rt = AsyncRuntime::new()?;

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
        let encryption_key = key_derivation::derive_persistence_encryption_key(&config.seed)?;
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

        // Step 11: Initialize the RapidSyncClient
        let rapid_sync = Arc::new(RapidGossipSync::new(
            Arc::clone(&graph),
            Arc::clone(&logger),
        ));
        let rapid_sync_client = Arc::new(RapidSyncClient::new(
            config.rgs_url.clone(),
            Arc::clone(&rapid_sync),
        )?);

        // Step 12: Initialize the ProbabilisticScorer
        let scorer = Arc::new(Mutex::new(
            persister.read_or_init_scorer(Arc::clone(&graph), Arc::clone(&logger))?,
        ));

        // Step 13: Initialize the Router
        let max_routing_fee_strategy = Arc::new(SimpleMaxRoutingFeeStrategy::new(21_000, 50));
        let score_params = ProbabilisticScoringFeeParameters {
            base_penalty_amount_multiplier_msat: 8192 * 50,
            base_penalty_msat: 500 * 50,
            ..Default::default()
        };
        let router = Arc::new(FeeLimitingRouter::new(
            Router::new(
                Arc::clone(&graph),
                Arc::clone(&logger),
                keys_manager.get_secure_random_bytes(),
                Arc::clone(&scorer),
                score_params,
            ),
            Arc::clone(&max_routing_fee_strategy),
        ));

        // (needed when using Electrum or BIP 157/158)
        // Step 14: Prepare ChannelMonitors for chain sync
        for (_, channel_monitor) in channel_monitors.iter() {
            channel_monitor.load_outputs_to_watch(&tx_sync);
        }

        // Step 15: Initialize the ChannelManager
        let mobile_node_user_config = build_mobile_node_user_config();
        let genesis_block = BestBlock::from_network(config.network);
        let chain_params = ChainParameters {
            network: config.network,
            best_block: genesis_block,
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

        // Step 16. Sync ChannelMonitors and ChannelManager to chain tip
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

        // Step 17. Give ChannelMonitors to ChainMonitor
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

        // Step 18. Initialize the PeerManager
        let peer_manager = Arc::new(init_peer_manager(
            Arc::clone(&channel_manager),
            Arc::clone(&keys_manager),
            Arc::clone(&logger),
        )?);

        // Step 19: Initialize the LspClient
        let lsp_client = Arc::new(LspClient::new(
            config.lsp_url.clone(),
            config.lsp_token.clone(),
        )?);

        // Step 20: Initialize the DataStore
        let data_store_path = Path::new(&config.local_persistence_path).join("db.db3");
        let data_store_path = data_store_path
            .to_str()
            .ok_or_invalid_input("Invalid local persistence path")?;
        let data_store = Arc::new(Mutex::new(DataStore::new(
            data_store_path,
            config.timezone_config.clone(),
        )?));

        // Step 21: Initialize the TaskManager
        let task_manager = Arc::new(Mutex::new(TaskManager::new(
            rt.handle(),
            Arc::clone(&lsp_client),
            Arc::clone(&peer_manager),
            Arc::clone(&fee_estimator),
            rapid_sync_client,
            Arc::clone(&channel_manager),
            Arc::clone(&chain_monitor),
            Arc::clone(&tx_sync),
            exchange_rate_provider,
            Arc::clone(&data_store),
        )?));
        task_manager
            .lock()
            .unwrap()
            .restart(get_foreground_periods());

        // Step 22. Initialize an EventHandler
        let event_handler = Arc::new(LipaEventHandler::new(
            Arc::clone(&channel_manager),
            Arc::clone(&task_manager),
            user_event_handler,
            Arc::clone(&data_store),
        )?);

        // Step 23. Start Background Processing
        // The fact that we do not restart the background process assumes that
        // it will never fail. However it may fail:
        //  1. on persisting channel manager, but it never fails since we ignore
        //     such failures in StoragePersister::persist_manager()
        //  2. on persisting scorer or network graph on exit, but we do not care
        // The other strategy to handle errors and restart the process will be
        // more difficult but will not provide any benefits.
        let background_processor = BackgroundProcessor::start(
            persister,
            Arc::clone(&event_handler),
            chain_monitor,
            Arc::clone(&channel_manager),
            GossipSync::rapid(rapid_sync),
            Arc::clone(&peer_manager),
            logger,
            Some(scorer),
        );

        Ok(Self {
            config: Mutex::new(config),
            rt,
            lsp_client,
            keys_manager,
            background_processor,
            channel_manager: Arc::clone(&channel_manager),
            peer_manager,
            task_manager,
            data_store,
            max_routing_fee_strategy,
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        let peer_pubkeys = self
            .peer_manager
            .get_peer_node_ids()
            .iter()
            .map(|p| p.0)
            .collect();

        let channels_info = get_channels_info(&self.channel_manager.list_channels());
        NodeInfo {
            node_pubkey: self.channel_manager.get_our_node_id(),
            peers: peer_pubkeys,
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
    ) -> Result<Bolt11Invoice> {
        trace!(
            "create_invoice() - called with:\n   amount_msat: {}\n   description: {}, metadata: {}",
            amount_msat,
            description,
            metadata
        );
        let (currency, fiat_currency) = {
            let config = self.config.lock().unwrap();
            let currency = match config.network {
                Network::Bitcoin => Currency::Bitcoin,
                Network::Testnet => Currency::BitcoinTestnet,
                Network::Regtest => Currency::Regtest,
                Network::Signet => Currency::Signet,
            };
            (currency, config.fiat_currency.clone())
        };
        let exchage_rates = self.task_manager.lock().unwrap().get_exchange_rates();

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
            &mut self.data_store.lock().unwrap(),
            &fiat_currency,
            exchage_rates,
        ))?;
        Bolt11Invoice::from_signed(signed_invoice)
            .map_to_permanent_failure("Failed to construct invoice")
    }

    pub fn decode_invoice(
        &self,
        invoice: String,
    ) -> std::result::Result<Bolt11Invoice, DecodeInvoiceError> {
        let network = self.config.lock().unwrap().network;
        invoice::decode_invoice(&invoice, network)
    }

    pub fn get_payment_max_routing_fee_mode(&self, amount_msat: u64) -> MaxRoutingFeeMode {
        self.max_routing_fee_strategy
            .get_payment_max_fee_mode(amount_msat)
    }

    pub fn pay_invoice(&self, invoice: String, metadata: String) -> PayResult<()> {
        trace!("pay_invoice() - called with invoice {}", invoice);
        let network = self.config.lock().unwrap().network;
        let invoice =
            invoice::decode_invoice(&invoice, network).map_to_invalid_input("Invalid invoice")?;

        let amount_msat = invoice.amount_milli_satoshis().unwrap_or(0);
        if amount_msat == 0 {
            return Err(invalid_input(
                "Expected invoice with a specified amount, but an open invoice was provided",
            ));
        }

        self.validate_persist_new_outgoing_payment_attempt(&invoice, amount_msat, &metadata)?;
        trace!("pay_invoice() - persisted new payment attempt in db");

        self.log_node_state();
        let payment_result = pay_invoice(
            &invoice,
            Retry::Timeout(PAYMENT_TIMEOUT),
            &self.channel_manager,
        );

        match payment_result {
            Ok(_payment_id) => {
                info!("pay_invoice() - Successfully initiated payment attempt for {amount_msat} msats");
                Ok(())
            }
            Err(e) => {
                error!("pay_invoice() - Failed to start payment attempt - {:?}", e);
                self.process_failed_payment_attempts(e, &invoice.payment_hash().to_string())
            }
        }
    }

    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_msat: u64,
        metadata: String,
    ) -> PayResult<()> {
        trace!("pay_open_invoice() - called with invoice {}", invoice);
        let network = self.config.lock().unwrap().network;
        let invoice =
            invoice::decode_invoice(&invoice, network).map_to_invalid_input("Invalid invoice")?;

        let invoice_amount_msat = invoice.amount_milli_satoshis().unwrap_or(0);
        if invoice_amount_msat != 0 {
            return Err(invalid_input(
                "Expected open invoice, but an invoice with a specified amount was provided",
            ));
        }
        if amount_msat == 0 {
            return Err(invalid_input(
                "Invoice does not specify an amount and no amount was specified manually",
            ));
        }

        self.validate_persist_new_outgoing_payment_attempt(&invoice, amount_msat, &metadata)?;
        trace!("pay_open_invoice() - persisted new payment attempt in db");

        self.log_node_state();
        let payment_result = pay_zero_value_invoice(
            &invoice,
            amount_msat,
            Retry::Timeout(PAYMENT_TIMEOUT),
            &self.channel_manager,
        );

        match payment_result {
            Ok(_payment_id) => {
                info!("pay_open_invoice() - Successfully initiated payment attempt for {amount_msat} msats (open amount invoice)");
                Ok(())
            }
            Err(e) => {
                error!(
                    "pay_open_invoice() - Failed to start payment attempt - {:?}",
                    e
                );
                self.process_failed_payment_attempts(e, &invoice.payment_hash().to_string())
            }
        }
    }

    pub fn lnurl_withdraw(&self, lnurlw: &str, amount_msat: u64) -> Result<String> {
        let client = lnurl::Builder {
            proxy: None,
            timeout: Some(10_000),
        }
        .build_blocking()
        .map_to_permanent_failure("Failed to build LNURL client")?;
        let response = client
            .make_request(lnurlw)
            .map_to_runtime_error(RuntimeErrorCode::LNURLError, "Failed to query server")?;
        let response = match response {
            LnUrlResponse::LnUrlPayResponse(_) => Err(runtime_error(
                RuntimeErrorCode::LNURLError,
                "Expected Withdraw response, got Pay resposne",
            )),
            LnUrlResponse::LnUrlWithdrawResponse(response) => Ok(response),
            LnUrlResponse::LnUrlChannelResponse(_) => Err(runtime_error(
                RuntimeErrorCode::LNURLError,
                "Expected Withdraw response, got Channel response",
            )),
        }?;

        let min = response.min_withdrawable.unwrap_or(1);
        let max = response.max_withdrawable;
        if amount_msat < min {
            return Err(runtime_error(
                RuntimeErrorCode::LNURLError,
                format!("Expected: {amount_msat} msat is below min withdrawable: {min} msat"),
            ));
        } else if amount_msat > max {
            return Err(runtime_error(
                RuntimeErrorCode::LNURLError,
                format!("Expected: {amount_msat} msat is above max withdrawable: {max} msat"),
            ));
        }

        let description = String::new();
        let metadata = String::new();
        let invoice = self.create_invoice(response.max_withdrawable, description, metadata)?;

        match client.do_withdrawal(&response, &invoice.to_string()) {
            Ok(Response::Ok { .. }) => Ok(()),
            Ok(Response::Error { reason }) => Err(runtime_error(
                RuntimeErrorCode::LNURLError,
                format!("Failed to withdraw: {reason}"),
            )),
            Err(e) => Err(runtime_error(
                RuntimeErrorCode::LNURLError,
                format!("Failed to withdraw: {e}"),
            )),
        }?;
        Ok(invoice.payment_hash().to_hex())
    }

    fn process_failed_payment_attempts(
        &self,
        error: PaymentError,
        payment_hash: &str,
    ) -> PayResult<()> {
        let (error, error_code) = match error {
            PaymentError::Invoice(e) => (
                Err(invalid_input(format!("Invalid invoice - {e}"))),
                PayErrorCode::UnexpectedError,
            ),
            PaymentError::Sending(e) => match e {
                RetryableSendFailure::PaymentExpired => (
                    Err(runtime_error(
                        PayErrorCode::InvoiceExpired,
                        "Invoice has expired",
                    )),
                    PayErrorCode::InvoiceExpired,
                ),
                RetryableSendFailure::RouteNotFound => (
                    Err(runtime_error(
                        PayErrorCode::NoRouteFound,
                        "Failed to find any route",
                    )),
                    PayErrorCode::NoRouteFound,
                ),
                RetryableSendFailure::DuplicatePayment => (
                    Err(permanent_failure("Duplicate payment")),
                    PayErrorCode::UnexpectedError,
                ),
            },
        };
        self.data_store
            .lock()
            .unwrap()
            .outgoing_payment_failed(payment_hash, error_code)
            .map_to_permanent_failure("Failed to persist payment result")?;
        error
    }

    fn validate_persist_new_outgoing_payment_attempt(
        &self,
        invoice: &Bolt11Invoice,
        amount_msat: u64,
        metadata: &str,
    ) -> PayResult<()> {
        let mut data_store = self.data_store.lock().unwrap();
        if let Ok(payment) = data_store.get_payment(&invoice.payment_hash().to_string()) {
            match payment.payment_type {
                PaymentType::Receiving => {
                    error!("Attempted to pay an invoice that was issued by the local node");
                    return Err(runtime_error(
                        PayErrorCode::PayingToSelf,
                        "This invoice was issued by the local node. Paying yourself is not supported.",
                    ));
                }
                PaymentType::Sending => {
                    if payment.payment_state != PaymentState::Failed {
                        error!("Attempted to pay an invoice that either has already been paid or for which a payment attempt is in progress - PaymentState: {:?}", payment.payment_state);
                        return Err(runtime_error(
                            PayErrorCode::AlreadyUsedInvoice,
                            "This invoice has already been paid or is in the process of being paid. Please use a different one or wait until the current payment attempt fails before retrying.",
                        ));
                    }
                    trace!("Starting a new attempt to pay an invoice which we weren't able to pay in the past");
                    data_store
                        .new_payment_state(
                            &invoice.payment_hash().to_string(),
                            PaymentState::Retried,
                        )
                        .map_to_permanent_failure("Failed to persist outgoing payment")?;
                    let fiat_currency = self.config.lock().unwrap().fiat_currency.clone();
                    let exchange_rates = self.task_manager.lock().unwrap().get_exchange_rates();
                    data_store
                        .update_payment_data(
                            &invoice.payment_hash().to_string(),
                            amount_msat,
                            metadata,
                            &fiat_currency,
                            exchange_rates,
                        )
                        .map_to_permanent_failure("Failed to persist updated payment data")?;
                }
            }
        } else {
            trace!("Starting our first attempt to pay an invoice");
            let description = match invoice.description() {
                Bolt11InvoiceDescription::Direct(d) => d.clone().into_inner(),
                Bolt11InvoiceDescription::Hash(h) => h.0.to_hex(),
            };
            let fiat_currency = self.config.lock().unwrap().fiat_currency.clone();
            let exchange_rates = self.task_manager.lock().unwrap().get_exchange_rates();
            data_store
                .new_outgoing_payment(
                    &invoice.payment_hash().to_string(),
                    amount_msat,
                    &description,
                    &invoice.to_string(),
                    metadata,
                    &fiat_currency,
                    exchange_rates,
                )
                .map_to_permanent_failure("Failed to persist outgoing payment")?;
        }
        Ok(())
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        if number_of_payments < 1 {
            error!("get_latest_payments() - called with number_of_payments set to 0");
            return Err(invalid_input(
                "Number of requested payments must be greater than 0",
            ));
        }
        self.data_store
            .lock()
            .unwrap()
            .get_latest_payments(number_of_payments)
    }

    pub fn get_payment(&self, hash: &str) -> Result<Payment> {
        self.data_store.lock().unwrap().get_payment(hash)
    }

    pub fn foreground(&self) {
        trace!("foreground() - called");
        self.task_manager
            .lock()
            .unwrap()
            .restart(get_foreground_periods());
    }

    pub fn background(&self) {
        trace!("background() - called");
        self.task_manager
            .lock()
            .unwrap()
            .restart(BACKGROUND_PERIODS);
    }

    pub fn list_exchange_rates(&self) -> Vec<ExchangeRate> {
        self.task_manager.lock().unwrap().get_exchange_rates()
    }

    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        let rates = self.task_manager.lock().unwrap().get_exchange_rates();
        let currency_code = self.config.lock().unwrap().fiat_currency.clone();
        rates
            .iter()
            .find(|r| r.currency_code == currency_code)
            .cloned()
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) {
        trace!(
            "change_fiat_currency() - called with fiat_currency: {}",
            fiat_currency
        );
        let mut task_manager = self.task_manager.lock().unwrap();
        self.config.lock().unwrap().fiat_currency = fiat_currency;
        // if the fiat currency is being changed, we can assume the app is in the foreground
        task_manager.restart(get_foreground_periods());
    }

    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
        trace!(
            "change_timezone_config() - called with timezone_config: {:?}",
            timezone_config
        );
        let mut data_store = self.data_store.lock().unwrap();
        data_store.update_timezone_config(timezone_config);
    }

    pub fn get_payment_amount_limits(&self) -> Result<PaymentAmountLimits> {
        let lsp_min_fee_msat = self.query_lsp_fee()?.channel_minimum_fee_msat;
        let inbound_capacity_msat = self.get_node_info().channels_info.inbound_capacity_msat;

        Ok(PaymentAmountLimits::calculate(
            inbound_capacity_msat,
            lsp_min_fee_msat,
        ))
    }

    // This implementation assumes that we don't ever spend the outputs provided by LDK
    // For now this is a reasonable assumption because we don't implement a way to spend them
    // TODO: as soon as it's possible to spend outputs persisted in the data store, update this
    //      implementation such that spent outputs are not contemplated
    pub fn get_onchain_balance(&self) -> Result<u64> {
        let mut balance = 0;

        let outputs = self
            .data_store
            .lock()
            .unwrap()
            .get_all_spendable_outputs()?;

        for output_descriptor in outputs {
            match output_descriptor {
                SpendableOutputDescriptor::StaticOutput {
                    outpoint: _,
                    output,
                } => {
                    balance += output.value;
                }
                SpendableOutputDescriptor::DelayedPaymentOutput(output_descriptor) => {
                    balance += output_descriptor.output.value;
                }
                SpendableOutputDescriptor::StaticPaymentOutput(output_descriptor) => {
                    balance += output_descriptor.output.value;
                }
            }
        }

        Ok(balance)
    }

    fn log_node_state(&self) {
        let node_info = self.get_node_info();

        trace!(
            "Logging node state...\n\
            Connected peers: {:?}\n\
            Number of channels: {}\n\
            Number of usable channels: {}\n\
            Local balance msat: {}\n\
            Inbound capacity msat: {}\n\
            Outbound capacity msat: {}\n\
            Capacity of all channels msat: {}",
            node_info.peers,
            node_info.channels_info.num_channels,
            node_info.channels_info.num_usable_channels,
            node_info.channels_info.local_balance_msat,
            node_info.channels_info.inbound_capacity_msat,
            node_info.channels_info.outbound_capacity_msat,
            node_info.channels_info.total_channel_capacities_msat
        );
        trace!("Per-channel state:");
        let channels = self.channel_manager.list_channels();
        for channel in channels {
            trace!(
                "Channel {}:\n\
                Usable: {}\n\
                Channel size sat: {}\n\
                Outbound capacity msat: {}\n\
                Inbound capacity msat: {}\n\
                Next outbound htlc limit msat: {}",
                channel.channel_id.to_hex(),
                channel.is_usable,
                channel.channel_value_satoshis,
                channel.outbound_capacity_msat,
                channel.inbound_capacity_msat,
                channel.next_outbound_htlc_limit_msat
            );
        }
    }

    pub fn sign_message(&self, message: &str) -> Result<String> {
        sign(message.as_bytes(), &self.keys_manager.get_node_secret_key())
            .map_to_permanent_failure("Failed to sign message")
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
    user_config.channel_config.max_dust_htlc_exposure =
        MaxDustHTLCExposure::FixedLimitMsat(1_000_000_000);

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
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_to_permanent_failure("Failed to get duration from Unix epoch")?;
    Ok(PeerManager::new_channel_only(
        channel_manager,
        IgnoringMessageHandler {},
        timestamp.as_secs() as u32,
        ephemeral_bytes.as_ref(),
        logger,
        keys_manager,
    ))
}

fn get_foreground_periods() -> TaskPeriods {
    match std::env::var("TESTING_TASK_PERIODS") {
        Ok(period) => {
            let period: u64 = period
                .parse()
                .expect("TESTING_TASK_PERIODS should be an integer number");
            let period = Duration::from_secs(period);
            TaskPeriods {
                sync_blockchain: period,
                update_lsp_info: Some(PeriodConfig {
                    success_period: period,
                    failure_period: period,
                }),
                reconnect_to_lsp: period,
                update_fees: Some(period),
                update_graph: Some(period),
                update_exchange_rates: Some(period),
            }
        }
        Err(_) => FOREGROUND_PERIODS,
    }
}
