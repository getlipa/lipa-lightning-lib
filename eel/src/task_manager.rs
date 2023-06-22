use crate::async_runtime::{Handle, RepeatingTaskHandle};
use crate::data_store::DataStore;
use crate::errors::Result;
use crate::fee_estimator::FeeEstimator;
use crate::interfaces::{ExchangeRate, ExchangeRateProvider};
use crate::lsp::{LspClient, LspInfo};
use crate::p2p_networking::{connect_peer, LnPeer};
use crate::rapid_sync_client::RapidSyncClient;
use crate::types::{ChainMonitor, ChannelManager, KeysManager, PeerManager, TxSync};
use crate::wallet::Wallet;

use crate::tx_broadcaster::TxBroadcaster;
use bdk::esplora_client::BlockingClient;
use bitcoin::Txid;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator as LdkFeeEstimator,
};
use lightning::chain::keysinterface::SpendableOutputDescriptor;
use lightning::chain::Confirm;
use log::{debug, error, info};
use secp256k1::SECP256K1;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::time::Duration;

pub(crate) type RestartIfFailedPeriod = Duration;

pub(crate) struct PeriodConfig {
    pub failure_period: Duration,
    pub success_period: Duration,
}

pub(crate) struct TaskPeriods {
    pub sync_blockchain: Duration,
    pub update_lsp_info: Option<PeriodConfig>,
    pub reconnect_to_lsp: Duration,
    pub update_fees: Option<Duration>,
    pub update_graph: Option<RestartIfFailedPeriod>,
    pub update_exchange_rates: Option<Duration>,
    pub sync_onchain_wallet: Option<Duration>,
    pub process_spendable_outputs: Option<Duration>,
}

pub(crate) struct TaskManager {
    runtime_handle: Handle,
    lsp_client: Arc<LspClient>,
    peer_manager: Arc<PeerManager>,
    fee_estimator: Arc<FeeEstimator>,

    lsp_info: Arc<Mutex<Option<LspInfo>>>,

    rapid_sync_client: Arc<RapidSyncClient>,

    channel_manager: Arc<ChannelManager>,
    chain_monitor: Arc<ChainMonitor>,
    tx_sync: Arc<TxSync>,

    exchange_rate_provider: Arc<dyn ExchangeRateProvider>,
    exchange_rates: Arc<Mutex<Vec<ExchangeRate>>>,
    data_store: Arc<Mutex<DataStore>>,

    wallet: Arc<Wallet>,

    esplora_client: Arc<BlockingClient>,
    keys_manager: Arc<KeysManager>,
    tx_broadcaster: Arc<TxBroadcaster>,

    task_handles: Vec<RepeatingTaskHandle>,
}

impl TaskManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_handle: Handle,
        lsp_client: Arc<LspClient>,
        peer_manager: Arc<PeerManager>,
        fee_estimator: Arc<FeeEstimator>,
        rapid_sync_client: Arc<RapidSyncClient>,
        channel_manager: Arc<ChannelManager>,
        chain_monitor: Arc<ChainMonitor>,
        tx_sync: Arc<TxSync>,
        exchange_rate_provider: Box<dyn ExchangeRateProvider>,
        data_store: Arc<Mutex<DataStore>>,
        esplora_client: Arc<BlockingClient>,
        keys_manager: Arc<KeysManager>,
        tx_broadcaster: Arc<TxBroadcaster>,
        wallet: Arc<Wallet>,
    ) -> Result<Self> {
        let exchange_rates = data_store.lock().unwrap().get_all_exchange_rates()?;

        Ok(Self {
            runtime_handle,
            lsp_client,
            peer_manager,
            fee_estimator,
            lsp_info: Arc::new(Mutex::new(None)),
            rapid_sync_client,
            channel_manager,
            chain_monitor,
            tx_sync,
            exchange_rate_provider: Arc::from(exchange_rate_provider),
            exchange_rates: Arc::new(Mutex::new(exchange_rates)),
            data_store,
            wallet,
            esplora_client,
            keys_manager,
            tx_broadcaster,
            task_handles: Vec::new(),
        })
    }

    pub fn get_lsp_info(&self) -> Option<LspInfo> {
        (*self.lsp_info.lock().unwrap()).clone()
    }

    pub fn get_exchange_rates(&self) -> Vec<ExchangeRate> {
        (*self.exchange_rates.lock().unwrap()).clone()
    }

    pub fn request_shutdown_all(&mut self) {
        self.task_handles
            .drain(..)
            .for_each(|h| h.request_shutdown());
    }

    pub fn restart(&mut self, periods: TaskPeriods) {
        self.request_shutdown_all();

        // Blockchain sync.
        self.task_handles
            .push(self.start_blockchain_sync(periods.sync_blockchain));

        // LSP info update.
        if let Some(config) = periods.update_lsp_info {
            self.task_handles.push(self.start_lsp_info_update(config));
        }

        // Reconnect to LSP LN node.
        self.task_handles
            .push(self.start_reconnect_to_lsp(periods.reconnect_to_lsp));

        // Update on-chain fee.
        if let Some(period) = periods.update_fees {
            self.task_handles.push(self.start_fee_update(period));
        }

        // Update network graph.
        // We regularly retry to update the network graph if it fails.
        // After the first successful update, not further updates are tried
        // until the app gets to the foreground again.
        if let Some(period) = periods.update_graph {
            self.task_handles.push(self.start_graph_update(period));
        }

        // Update exchange rates.
        if let Some(period) = periods.update_exchange_rates {
            self.task_handles
                .push(self.start_exchange_rate_update(period));
        }

        // Sync onchain wallet
        if let Some(period) = periods.sync_onchain_wallet {
            self.task_handles
                .push(self.start_onchain_wallet_sync(period))
        }

        if let Some(period) = periods.process_spendable_outputs {
            self.task_handles
                .push(self.start_spendable_output_processing(period))
        }
    }

    fn start_blockchain_sync(&self, period: Duration) -> RepeatingTaskHandle {
        let channel_manager = Arc::clone(&self.channel_manager);
        let chain_monitor = Arc::clone(&self.chain_monitor);
        let tx_sync = Arc::clone(&self.tx_sync);

        self.runtime_handle.spawn_repeating_task(period, move || {
            let tx_sync = Arc::clone(&tx_sync);
            let channel_manager_regular_sync = Arc::clone(&channel_manager);
            let chain_monitor_regular_sync = Arc::clone(&chain_monitor);
            async move {
                let confirmables = vec![
                    &*channel_manager_regular_sync as &(dyn Confirm + Sync + Send),
                    &*chain_monitor_regular_sync as &(dyn Confirm + Sync + Send),
                ];
                let now = Instant::now();
                match tx_sync.sync(confirmables).await {
                    Ok(()) => debug!(
                        "Sync to blockchain finished in {}ms",
                        now.elapsed().as_millis()
                    ),
                    Err(e) => error!("Sync to blockchain failed: {:?}", e),
                }
            }
        })
    }

    fn start_lsp_info_update(&self, config: PeriodConfig) -> RepeatingTaskHandle {
        let peer_manager = Arc::clone(&self.peer_manager);
        let lsp_client = Arc::clone(&self.lsp_client);
        let lsp_info = Arc::clone(&self.lsp_info);
        self.runtime_handle.spawn_self_restarting_task(move || {
            let peer_manager = Arc::clone(&peer_manager);
            let lsp_client = Arc::clone(&lsp_client);
            let lsp_info = Arc::clone(&lsp_info);
            async move {
                match lsp_client.query_info().await {
                    Ok(new_lsp_info) => {
                        if Some(new_lsp_info.clone()) != *lsp_info.lock().unwrap() {
                            debug!("New LSP info received: {:?}", new_lsp_info);
                            *lsp_info.lock().unwrap() = Some(new_lsp_info.clone());

                            // Kick in reconnecting to LSP when we get new info.
                            let peer = LnPeer {
                                pub_key: new_lsp_info.node_info.pubkey,
                                host: new_lsp_info.node_info.address,
                            };
                            if let Err(e) = connect_peer(&peer, peer_manager).await {
                                error!("Connecting to peer {} failed: {}", peer, e);
                            }
                        }
                        Some(config.success_period)
                    }
                    Err(e) => {
                        error!("Failed to query LSP, retrying in 10 seconds: {}", e);
                        Some(config.failure_period)
                    }
                }
            }
        })
    }

    fn start_reconnect_to_lsp(&self, period: Duration) -> RepeatingTaskHandle {
        let peer_manager = Arc::clone(&self.peer_manager);
        let lsp_info = Arc::clone(&self.lsp_info);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let peer_manager = Arc::clone(&peer_manager);
            let lsp_info = Arc::clone(&lsp_info);
            async move {
                let lsp_info = (*lsp_info.lock().unwrap()).clone();
                if let Some(lsp_info) = lsp_info {
                    let peer = LnPeer {
                        pub_key: lsp_info.node_info.pubkey,
                        host: lsp_info.node_info.address,
                    };
                    if let Err(e) = connect_peer(&peer, peer_manager).await {
                        error!("Connecting to peer {} failed: {}", peer, e);
                    }
                }
            }
        })
    }

    fn start_fee_update(&self, period: Duration) -> RepeatingTaskHandle {
        let fee_estimator = Arc::clone(&self.fee_estimator);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let fee_estimator = Arc::clone(&fee_estimator);
            async move {
                match tokio::task::spawn_blocking(move || fee_estimator.poll_updates()).await {
                    Ok(Ok(())) => (),
                    Ok(Err(e)) => error!("Failed to get fee estimates: {}", e),
                    Err(e) => error!("Update fees task panicked: {}", e),
                }
            }
        })
    }

    fn start_graph_update(&self, period: RestartIfFailedPeriod) -> RepeatingTaskHandle {
        let rapid_sync_client = Arc::clone(&self.rapid_sync_client);
        self.runtime_handle.spawn_self_restarting_task(move || {
            let rapid_sync_client = Arc::clone(&rapid_sync_client);
            async move {
                match tokio::task::spawn_blocking(move || rapid_sync_client.sync()).await {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => {
                        error!("Failed to update network graph: {}", e);
                        Some(period)
                    }
                    Err(e) => {
                        error!("Update graph task panicked: {}", e);
                        Some(period)
                    }
                }
            }
        })
    }

    fn start_exchange_rate_update(&self, period: Duration) -> RepeatingTaskHandle {
        let exchange_rate_provider = Arc::clone(&self.exchange_rate_provider);
        let exchange_rates = Arc::clone(&self.exchange_rates);
        let data_store = Arc::clone(&self.data_store);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let exchange_rate_provider = Arc::clone(&exchange_rate_provider);
            let exchange_rates = Arc::clone(&exchange_rates);
            let data_store = Arc::clone(&data_store);
            async move {
                match tokio::task::spawn_blocking(move || {
                    exchange_rate_provider.query_all_exchange_rates()
                })
                .await
                {
                    Ok(Ok(rates)) => {
                        persist_exchange_rates(&data_store, &rates);
                        *exchange_rates.lock().unwrap() = rates;
                    }
                    Ok(Err(e)) => {
                        error!("Failed to update exchange rates: {e}");
                    }
                    Err(e) => {
                        error!("Update exchange rates task panicked: {e}");
                    }
                }
            }
        })
    }

    fn start_onchain_wallet_sync(&self, period: Duration) -> RepeatingTaskHandle {
        let wallet = Arc::clone(&self.wallet);

        self.runtime_handle.spawn_repeating_task(period, move || {
            let wallet = Arc::clone(&wallet);
            async move {
                match tokio::task::spawn_blocking(move || wallet.sync()).await {
                    Ok(Ok(_)) => {
                        info!("Onchain wallet synced successfully!")
                    }
                    Ok(Err(e)) => {
                        error!("Failed to sync onchain wallet: {e}");
                    }
                    Err(e) => {
                        error!("Sync onchain wallet task panicked: {e}");
                    }
                }
            }
        })
    }

    fn start_spendable_output_processing(&self, period: Duration) -> RepeatingTaskHandle {
        let wallet = Arc::clone(&self.wallet);
        let data_store = Arc::clone(&self.data_store);
        let esplora_client = Arc::clone(&self.esplora_client);
        let keys_manager = Arc::clone(&self.keys_manager);
        let fee_estimator = Arc::clone(&self.fee_estimator);
        let tx_broadcaster = Arc::clone(&self.tx_broadcaster);

        self.runtime_handle.spawn_repeating_task(period, move || {
            let wallet = Arc::clone(&wallet);
            let data_store = Arc::clone(&data_store);
            let esplora_client = Arc::clone(&esplora_client);
            let keys_manager = Arc::clone(&keys_manager);
            let fee_estimator = Arc::clone(&fee_estimator);
            let tx_broadcaster = Arc::clone(&tx_broadcaster);

            async move {
                // get non spent spendable outputs
                let pending_outputs = data_store
                    .lock()
                    .unwrap()
                    .get_pending_spendable_outputs()
                    .unwrap();

                let to_spend_outputs = pending_outputs
                    .iter()
                    .filter(|output| is_unspent(output, &esplora_client))
                    .collect::<Vec<_>>();
                if !to_spend_outputs.is_empty() {
                    let destination_address = wallet.get_new_address().unwrap();
                    let tx_feerate =
                        fee_estimator.get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
                    let res = keys_manager.spend_spendable_outputs(
                        &to_spend_outputs,
                        Vec::new(),
                        destination_address.script_pubkey(),
                        tx_feerate,
                        SECP256K1,
                    );
                    match res {
                        Ok(spending_tx) => tx_broadcaster.broadcast_transaction(&spending_tx),
                        Err(err) => {
                            error!("Error spending outputs: {:?}", err);
                        }
                    }
                    info!(
                        "Successfully broadcasted a tx spending {} non-static spendable outputs!",
                        to_spend_outputs.len()
                    );
                }

                let data_store = data_store.lock().unwrap();
                pending_outputs
                    .iter()
                    .filter(|output| is_confirmed(output, &esplora_client))
                    .for_each(|output| {
                        data_store.confirm_pending_spendable_output(output).unwrap()
                    });
            }
        })
    }
}

fn is_unspent(
    spendable_output: &SpendableOutputDescriptor,
    _esplora_client: &Arc<BlockingClient>,
) -> bool {
    let (_txid, _index) = get_spendable_output_outpoint(spendable_output);

    // TODO: Return true if a tx that spends this outputs is unknown by the esplora server
    /*
    match esplora_client
        .get_output_status(&txid, index as u64)
        .unwrap()
    {
        None => true,
        Some(status) => status.status.is_none(),
    }
    */
    true
}

fn is_confirmed(
    spendable_output: &SpendableOutputDescriptor,
    _esplora_client: &Arc<BlockingClient>,
) -> bool {
    let (_txid, _index) = get_spendable_output_outpoint(spendable_output);

    // TODO: return true if a there's tx that spends this output that has a min of 6 confs
    true
}

fn get_spendable_output_outpoint(spendable_output: &SpendableOutputDescriptor) -> (Txid, u16) {
    match spendable_output {
        SpendableOutputDescriptor::StaticOutput { outpoint, .. } => (outpoint.txid, outpoint.index),
        SpendableOutputDescriptor::DelayedPaymentOutput(out) => {
            (out.outpoint.txid, out.outpoint.index)
        }
        SpendableOutputDescriptor::StaticPaymentOutput(out) => {
            (out.outpoint.txid, out.outpoint.index)
        }
    }
}

fn persist_exchange_rates(data_store: &Arc<Mutex<DataStore>>, rates: &[ExchangeRate]) {
    let data_store = data_store.lock().unwrap();
    for rate in rates {
        match data_store.update_exchange_rate(&rate.currency_code, rate.rate, rate.updated_at) {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to update exchange rate in db: {}", e)
            }
        }
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.request_shutdown_all();
    }
}
