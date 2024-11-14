use crate::amount::{AsSats, ToAmount};
use crate::errors::Result;
use crate::locker::Locker;
use crate::task_manager::TaskManager;
use crate::{ChannelsInfo, ExchangeRate, NodeInfo, RuntimeErrorCode, UserPreferences};
use breez_sdk_core::BreezServices;
use perro::MapToError;
use std::sync::{Arc, Mutex};

pub(crate) struct Support {
    user_preferences: Arc<Mutex<UserPreferences>>,
    task_manager: Arc<Mutex<TaskManager>>,
    sdk: Arc<BreezServices>,
}

impl Support {
    pub fn new(
        user_preferences: Arc<Mutex<UserPreferences>>,
        task_manager: Arc<Mutex<TaskManager>>,
        sdk: Arc<BreezServices>,
    ) -> Self {
        Self {
            user_preferences,
            task_manager,
            sdk,
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

    pub fn get_exchange_rates(&self) -> Vec<ExchangeRate> {
        self.task_manager.lock_unwrap().get_exchange_rates()
    }

    /// Request some basic info about the node
    ///
    /// Requires network: **no**
    pub fn get_node_info(&self) -> Result<NodeInfo> {
        let node_state = self.sdk.node_info().map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to read node info",
        )?;
        let rate = self.get_exchange_rate();

        Ok(NodeInfo {
            node_pubkey: node_state.id,
            peers: node_state.connected_peers,
            onchain_balance: node_state
                .onchain_balance_msat
                .as_msats()
                .to_amount_down(&rate),
            channels_info: ChannelsInfo {
                local_balance: node_state
                    .channels_balance_msat
                    .as_msats()
                    .to_amount_down(&rate),
                max_receivable_single_payment: node_state
                    .max_receivable_single_payment_amount_msat
                    .as_msats()
                    .to_amount_down(&rate),
                total_inbound_capacity: node_state
                    .total_inbound_liquidity_msats
                    .as_msats()
                    .to_amount_down(&rate),
                outbound_capacity: node_state.max_payable_msat.as_msats().to_amount_down(&rate),
            },
        })
    }
}
