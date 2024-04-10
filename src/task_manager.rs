use crate::async_runtime::{Handle, RepeatingTaskHandle};
use crate::data_store::{BackupStatus, DataStore};
use crate::errors::Result;
use crate::exchange_rate_provider::{ExchangeRate, ExchangeRateProvider};
use crate::locker::Locker;
use crate::{BreezHealthCheckStatus, EventsCallback, RuntimeErrorCode};
use std::env;

use crate::backup::BackupManager;
use breez_sdk_core::{BreezServices, OpeningFeeParams};
use log::{debug, error};
use perro::OptionToError;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;

pub(crate) struct TaskPeriods {
    pub update_exchange_rates: Option<Duration>,
    pub sync_breez: Option<Duration>,
    pub update_lsp_fee: Option<Duration>,
    pub backup: Option<Duration>,
    pub health_status_check: Option<Duration>,
}

pub(crate) struct TaskManager {
    runtime_handle: Handle,
    exchange_rate_provider: Arc<dyn ExchangeRateProvider>,
    exchange_rates: Arc<Mutex<Vec<ExchangeRate>>>,
    data_store: Arc<Mutex<DataStore>>,
    sdk: Arc<BreezServices>,
    lsp_fee: Arc<Mutex<Option<OpeningFeeParams>>>,
    backup_manager: Arc<BackupManager>,
    events_callback: Arc<Box<dyn EventsCallback>>,
    breez_health_status: Arc<Mutex<Option<BreezHealthCheckStatus>>>,

    task_handles: Vec<RepeatingTaskHandle>,
}

const FOREGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_exchange_rates: Some(Duration::from_secs(10 * 60)),
    sync_breez: Some(Duration::from_secs(10 * 60)),
    update_lsp_fee: Some(Duration::from_secs(10 * 60)),
    backup: Some(Duration::from_secs(30)),
    health_status_check: Some(Duration::from_secs(70)),
};

const BACKGROUND_PERIODS: TaskPeriods = TaskPeriods {
    update_exchange_rates: None,
    sync_breez: Some(Duration::from_secs(30 * 60)),
    update_lsp_fee: None,
    backup: None,
    health_status_check: None,
};
impl TaskManager {
    pub fn new(
        runtime_handle: Handle,
        exchange_rate_provider: Box<dyn ExchangeRateProvider>,
        data_store: Arc<Mutex<DataStore>>,
        sdk: Arc<BreezServices>,
        backup_manager: BackupManager,
        events_callback: Arc<Box<dyn EventsCallback>>,
    ) -> Result<Self> {
        let exchange_rates = data_store.lock_unwrap().get_all_exchange_rates()?;

        Ok(Self {
            runtime_handle,
            exchange_rate_provider: Arc::from(exchange_rate_provider),
            exchange_rates: Arc::new(Mutex::new(exchange_rates)),
            data_store,
            sdk,
            lsp_fee: Arc::new(Mutex::new(None)),
            backup_manager: Arc::new(backup_manager),
            events_callback,
            breez_health_status: Arc::new(Mutex::new(None)),
            task_handles: Vec::new(),
        })
    }

    pub fn get_exchange_rates(&self) -> Vec<ExchangeRate> {
        self.exchange_rates.lock_unwrap().clone()
    }

    pub fn get_lsp_fee(&self) -> Result<OpeningFeeParams> {
        self.lsp_fee.lock_unwrap().clone().ok_or_runtime_error(
            RuntimeErrorCode::LspServiceUnavailable,
            "Cached LSP fee isn't available",
        )
    }

    pub fn foreground(&mut self) {
        self.restart(get_foreground_periods());
    }

    pub fn background(&mut self) {
        self.restart(BACKGROUND_PERIODS)
    }

    pub fn request_shutdown_all(&mut self) {
        self.task_handles
            .drain(..)
            .for_each(|h| h.request_shutdown());
    }

    fn restart(&mut self, periods: TaskPeriods) {
        self.request_shutdown_all();

        // Update exchange rates.
        if let Some(period) = periods.update_exchange_rates {
            self.task_handles
                .push(self.start_exchange_rate_update(period));
        }

        // Sync breez sdk.
        if let Some(period) = periods.sync_breez {
            self.task_handles.push(self.start_breez_sync(period));
        }

        // Update lsp fee.
        if let Some(period) = periods.update_lsp_fee {
            self.task_handles.push(self.start_lsp_fee_update(period));
        }

        // Backup local db
        if let Some(period) = periods.backup {
            self.task_handles.push(self.start_backup_local_db(period));
        }

        // Health status check
        if let Some(period) = periods.health_status_check {
            self.task_handles
                .push(self.start_health_status_check(period));
        }
    }

    fn start_breez_sync(&self, period: Duration) -> RepeatingTaskHandle {
        let sdk = Arc::clone(&self.sdk);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let sdk = Arc::clone(&sdk);
            async move {
                debug!("Starting breez sdk sync");
                if let Err(e) = sdk.sync().await {
                    error!("Failed to sync breez sdk: {e}");
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
                debug!("Starting exchange rate update task");
                match tokio::task::spawn_blocking(move || {
                    exchange_rate_provider.query_all_exchange_rates()
                })
                .await
                {
                    Ok(Ok(rates)) => {
                        persist_exchange_rates(&data_store, &rates);
                        *exchange_rates.lock_unwrap() = rates;
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

    fn start_lsp_fee_update(&self, period: Duration) -> RepeatingTaskHandle {
        let sdk = Arc::clone(&self.sdk);
        let lsp_fee = Arc::clone(&self.lsp_fee);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let sdk = Arc::clone(&sdk);
            let lsp_fee = Arc::clone(&lsp_fee);

            async move {
                debug!("Starting lsp fee update task");
                match sdk.lsp_info().await {
                    Ok(lsp_information) => {
                        match lsp_information
                            .opening_fee_params_list
                            .get_cheapest_opening_fee_params()
                        {
                            Ok(opening_fee_params) => {
                                *lsp_fee.lock_unwrap() = Some(opening_fee_params);
                            }
                            Err(e) => {
                                error!("Failed to retrieve cheapest opening fee params: {e}");
                            }
                        };
                    }
                    Err(e) => {
                        error!("Failed to update lsp fee: {e}");
                    }
                }
            }
        })
    }

    fn start_backup_local_db(&self, period: Duration) -> RepeatingTaskHandle {
        let data_store = Arc::clone(&self.data_store);
        let backup_manager = Arc::clone(&self.backup_manager);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let data_store = Arc::clone(&data_store);
            let backup_manager = Arc::clone(&backup_manager);
            async move {
                debug!("Starting local db backup task");
                let backup_status = data_store.lock_unwrap().backup_status;
                match backup_status {
                    BackupStatus::Complete => {}
                    BackupStatus::WaitingForBackup => {
                        match data_store.lock_unwrap().backup_db() {
                            Ok(_) => {
                                debug!("Successfully backed up local db into separate db file");
                            }
                            Err(e) => {
                                error!("Failed to back up local db into separate db file: {e}");
                                return;
                            }
                        }
                        match backup_manager.backup().await {
                            Ok(_) => {
                                debug!("Successfully backed up local db to remote storage");
                                data_store.lock_unwrap().backup_status = BackupStatus::Complete;
                            }
                            Err(e) => {
                                error!("Failed to back up local db to remote storage: {e}")
                            }
                        }
                    }
                }
            }
        })
    }

    fn start_health_status_check(&self, period: Duration) -> RepeatingTaskHandle {
        let status = Arc::clone(&self.breez_health_status);
        let events_callback = Arc::clone(&self.events_callback);
        self.runtime_handle.spawn_repeating_task(period, move || {
            let status = Arc::clone(&status);
            let events_callback = Arc::clone(&events_callback);
            async move {
                debug!("Starting health status check task");
                let new_status = match BreezServices::service_health_check(
                    env!("BREEZ_SDK_API_KEY").to_string(),
                )
                .await
                {
                    Ok(status) => {
                        debug!("Breez Health Status: {status:?}");
                        status.status
                    }
                    Err(e) => {
                        error!("Failed to call service_health_check(): {e}");
                        return;
                    }
                };

                let some_new_status = Some(new_status.clone());
                if *status.lock_unwrap() != some_new_status {
                    *status.lock_unwrap() = some_new_status;
                    events_callback.breez_health_status_changed_to(new_status);
                }
            }
        })
    }
}

fn persist_exchange_rates(data_store: &Arc<Mutex<DataStore>>, rates: &[ExchangeRate]) {
    let mut data_store = data_store.lock_unwrap();
    for rate in rates {
        match data_store.update_exchange_rate(&rate.currency_code, rate.rate, rate.updated_at) {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to update exchange rate in db: {e}")
            }
        }
    }
}

fn get_foreground_periods() -> TaskPeriods {
    match env::var("TESTING_TASK_PERIODS") {
        Ok(period) => {
            let period: u64 = period
                .parse()
                .expect("TESTING_TASK_PERIODS should be an integer number");
            let period = Duration::from_secs(period);
            TaskPeriods {
                update_exchange_rates: Some(period),
                sync_breez: Some(period),
                update_lsp_fee: Some(period),
                backup: Some(period),
                health_status_check: Some(period),
            }
        }
        Err(_) => FOREGROUND_PERIODS,
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.request_shutdown_all();
    }
}
