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
use crate::data_store::DataStore;
use crate::errors::*;
use crate::esplora_client::EsploraClient;
use crate::event_handler::LipaEventHandler;
use crate::fee_estimator::FeeEstimator;
use crate::interfaces::{EventHandler, ExchangeRate, ExchangeRateProvider, RemoteStorage};
pub use crate::invoice::InvoiceDetails;
use crate::invoice::{create_invoice, validate_invoice, CreateInvoiceParams};
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

use bitcoin::hashes::hex::ToHex;
pub use bitcoin::Network;
use cipher::consts::U32;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::{
    EntropySource, InMemorySigner, KeysManager, SpendableOutputDescriptor,
};
use lightning::chain::{BestBlock, ChannelMonitorUpdateStatus, Confirm, Watch};
use lightning::ln::channelmanager::{ChainParameters, Retry, RetryableSendFailure};
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::util::config::UserConfig;
use lightning_background_processor::{BackgroundProcessor, GossipSync};
use lightning_invoice::payment::{pay_invoice, pay_zero_value_invoice, PaymentError};
use lightning_invoice::Currency;
pub use lightning_invoice::{Invoice, InvoiceDescription};
use log::error;
pub use log::Level as LogLevel;
use log::{info, warn};
pub use perro::{
    invalid_input, permanent_failure, runtime_error, MapToError, MapToErrorForUnitType,
    OptionToError,
};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::time::Duration;

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
}

impl LightningNode {
    pub fn new(
        config: Config,
        remote_storage: Box<dyn RemoteStorage>,
        user_event_handler: Box<dyn EventHandler>,
        exchange_rate_provider: Box<dyn ExchangeRateProvider>,
    ) -> Result<Self> {
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
        let router = Arc::new(Router::new(
            Arc::clone(&graph),
            Arc::clone(&logger),
            keys_manager.get_secure_random_bytes(),
            Arc::clone(&scorer),
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
        })
    }

    pub fn get_node_info(&self) -> NodeInfo {
        let channels_info = get_channels_info(&self.channel_manager.list_channels());
        NodeInfo {
            node_pubkey: self.channel_manager.get_our_node_id(),
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
    ) -> Result<Invoice> {
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
        Invoice::from_signed(signed_invoice).map_to_permanent_failure("Failed to construct invoice")
    }

    pub fn decode_invoice(&self, invoice: String) -> Result<Invoice> {
        let invoice = invoice::parse_invoice(&invoice)?;
        if self.config.lock().unwrap().network != invoice.network() {
            return Err(runtime_error(
                RuntimeErrorCode::InvoiceNetworkMismatch,
                format!(
                    "Invoice belongs to a different network: {}",
                    invoice.network()
                ),
            ));
        }
        Ok(invoice)
    }

    pub fn pay_invoice(&self, invoice: String, metadata: String) -> Result<()> {
        let invoice = invoice::parse_invoice(&invoice)?;
        let amount_msat = invoice.amount_milli_satoshis().unwrap_or(0);

        if amount_msat == 0 {
            return Err(invalid_input(
                "Expected invoice with a specified amount, but an open invoice was provided",
            ));
        }

        self.validate_persist_new_outgoing_payment_attempt(&invoice, amount_msat, &metadata)?;

        let payment_result = pay_invoice(
            &invoice,
            Retry::Timeout(Duration::from_secs(15)),
            &self.channel_manager,
        );

        match payment_result {
            Ok(_payment_id) => {
                info!("Initiated payment of {amount_msat} msats");
                Ok(())
            }
            Err(e) => self.process_failed_payment_attempts(e, &invoice.payment_hash().to_string()),
        }
    }

    pub fn pay_open_invoice(
        &self,
        invoice: String,
        amount_msat: u64,
        metadata: String,
    ) -> Result<()> {
        let invoice = invoice::parse_invoice(&invoice)?;
        let invoice_amount_msat = invoice.amount_milli_satoshis().unwrap_or(0);

        if invoice_amount_msat != 0 {
            return Err(invalid_input(
                "Expected open invoice, but an invoice with a specified amount was provided",
            ));
        } else if amount_msat == 0 {
            return Err(invalid_input(
                "Invoice does not specify an amount and no amount was specified manually",
            ));
        }

        self.validate_persist_new_outgoing_payment_attempt(&invoice, amount_msat, &metadata)?;

        let payment_result = pay_zero_value_invoice(
            &invoice,
            amount_msat,
            Retry::Timeout(Duration::from_secs(10)),
            &self.channel_manager,
        );

        match payment_result {
            Ok(_payment_id) => {
                info!("Initiated payment of {amount_msat} msats (open amount invoice)");
                Ok(())
            }
            Err(e) => self.process_failed_payment_attempts(e, &invoice.payment_hash().to_string()),
        }
    }

    fn process_failed_payment_attempts(
        &self,
        error: PaymentError,
        payment_hash: &str,
    ) -> Result<()> {
        match error {
            PaymentError::Invoice(e) => {
                self.data_store
                    .lock()
                    .unwrap()
                    .new_payment_state(payment_hash, PaymentState::Failed)?;
                Err(invalid_input(format!("Invalid invoice - {e}")))
            }
            PaymentError::Sending(e) => {
                self.data_store
                    .lock()
                    .unwrap()
                    .new_payment_state(payment_hash, PaymentState::Failed)?;
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

    fn validate_persist_new_outgoing_payment_attempt(
        &self,
        invoice: &Invoice,
        amount_msat: u64,
        metadata: &str,
    ) -> Result<()> {
        validate_invoice(self.config.lock().unwrap().network, invoice)?;

        let mut data_store = self.data_store.lock().unwrap();
        if let Ok(payment) = data_store.get_payment(&invoice.payment_hash().to_string()) {
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
                    data_store.new_payment_state(
                        &invoice.payment_hash().to_string(),
                        PaymentState::Retried,
                    )?;
                }
            }
        } else {
            let description = match invoice.description() {
                InvoiceDescription::Direct(d) => d.clone().into_inner(),
                InvoiceDescription::Hash(h) => h.0.to_hex(),
            };
            let fiat_currency = self.config.lock().unwrap().fiat_currency.clone();
            let exchange_rates = self.task_manager.lock().unwrap().get_exchange_rates();
            data_store.new_outgoing_payment(
                &invoice.payment_hash().to_string(),
                amount_msat,
                &description,
                &invoice.to_string(),
                metadata,
                &fiat_currency,
                exchange_rates,
            )?;
        }
        Ok(())
    }

    pub fn get_latest_payments(&self, number_of_payments: u32) -> Result<Vec<Payment>> {
        if number_of_payments < 1 {
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
        self.task_manager
            .lock()
            .unwrap()
            .restart(get_foreground_periods());
    }

    pub fn background(&self) {
        self.task_manager
            .lock()
            .unwrap()
            .restart(BACKGROUND_PERIODS);
    }

    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        let rates = self.task_manager.lock().unwrap().get_exchange_rates();
        rates
            .iter()
            .find(|r| r.currency_code == self.config.lock().unwrap().fiat_currency)
            .cloned()
    }

    pub fn change_fiat_currency(&self, fiat_currency: String) {
        let mut task_manager = self.task_manager.lock().unwrap();
        self.config.lock().unwrap().fiat_currency = fiat_currency;
        // if the fiat currency is being changed, we can assume the app is in the foreground
        task_manager.restart(get_foreground_periods());
    }

    pub fn change_timezone_config(&self, timezone_config: TzConfig) {
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

    pub fn panic_directly(&self) {
        panic_directly()
    }

    pub fn panic_in_background_thread(&self) {
        std::thread::spawn(panic_directly);
        sleep(Duration::from_secs(1));
    }

    pub fn panic_in_tokio(&self) {
        self.rt.handle().spawn(async { panic_directly() });
        sleep(Duration::from_secs(1));
    }
}

fn panic_directly() {
    let instant = Instant::now();
    let duration = Duration::from_secs(u64::MAX); // Max value of u64

    let _result = instant - duration;
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

#[cfg(test)]
mod tests {
    use crate::panic_directly;

    #[test]
    #[should_panic]
    fn test_panic_directly() {
        panic_directly();
    }
}
