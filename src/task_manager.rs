use crate::async_runtime::{Handle, RepeatingTaskHandle};
use crate::errors::Result;
pub use crate::exchange_rate_provider::{ExchangeRate, ExchangeRateProviderImpl};

use crate::data_store::DataStore;
use crate::exchange_rate_provider::ExchangeRateProvider;
use log::{error, trace};
use std::sync::{Arc, Mutex};
use tokio::time::Duration;

pub(crate) struct TaskPeriods {
    pub update_exchange_rates: Option<Duration>,
}

pub(crate) struct TaskManager {
    runtime_handle: Handle,
    exchange_rate_provider: Arc<dyn ExchangeRateProvider>,
    exchange_rates: Arc<Mutex<Vec<ExchangeRate>>>,
    data_store: Arc<Mutex<DataStore>>,

    task_handles: Vec<RepeatingTaskHandle>,
}

impl TaskManager {
    pub fn new(
        runtime_handle: Handle,
        exchange_rate_provider: Box<dyn ExchangeRateProvider>,
        data_store: Arc<Mutex<DataStore>>,
    ) -> Result<Self> {
        let exchange_rates = data_store.lock().unwrap().get_all_exchange_rates()?;

        Ok(Self {
            runtime_handle,
            exchange_rate_provider: Arc::from(exchange_rate_provider),
            exchange_rates: Arc::new(Mutex::new(exchange_rates)),
            data_store,
            task_handles: Vec::new(),
        })
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

        // Update exchange rates.
        if let Some(period) = periods.update_exchange_rates {
            self.task_handles
                .push(self.start_exchange_rate_update(period));
        }
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
                trace!("Starting exchange rate update task");
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
}

fn persist_exchange_rates(data_store: &Arc<Mutex<DataStore>>, rates: &[ExchangeRate]) {
    let data_store = data_store.lock().unwrap();
    for rate in rates {
        match data_store.update_exchange_rate(&rate.currency_code, rate.rate, rate.updated_at) {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to update exchange rate in db: {e}")
            }
        }
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.request_shutdown_all();
    }
}
