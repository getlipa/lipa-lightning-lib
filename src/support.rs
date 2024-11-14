use crate::locker::Locker;
use crate::task_manager::TaskManager;
use crate::{ExchangeRate, UserPreferences};
use std::sync::{Arc, Mutex};

pub(crate) struct Support {
    user_preferences: Arc<Mutex<UserPreferences>>,
    task_manager: Arc<Mutex<TaskManager>>,
}

impl Support {
    pub fn new(
        user_preferences: Arc<Mutex<UserPreferences>>,
        task_manager: Arc<Mutex<TaskManager>>,
    ) -> Self {
        Self {
            user_preferences,
            task_manager,
        }
    }

    /// Get exchange rate on the BTC/default currency pair
    /// Please keep in mind that this method doesn't make any network calls. It simply retrieves
    /// previously fetched values that are frequently updated by a background task.
    ///
    /// The fetched exchange rates will be persisted across restarts to alleviate the consequences of a
    /// slow or unresponsive exchange rate service.
    ///
    /// The return value is an optional to deal with the possibility
    /// of no exchange rate values being known.
    ///
    /// Requires network: **no**
    pub fn get_exchange_rate(&self) -> Option<ExchangeRate> {
        let rates = self.task_manager.lock_unwrap().get_exchange_rates();
        let currency_code = self.user_preferences.lock_unwrap().fiat_currency.clone();
        rates
            .iter()
            .find(|r| r.currency_code == currency_code)
            .cloned()
    }
}
