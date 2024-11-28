use crate::amount::{AsSats, ToAmount};
use crate::analytics::AnalyticsInterceptor;
use crate::async_runtime::AsyncRuntime;
use crate::data_store::DataStore;
use crate::errors::Result;
use crate::locker::Locker;
use crate::phone_number::PhoneNumberPrefixParser;
use crate::pocketclient::PocketClient;
use crate::task_manager::TaskManager;
use crate::util::LogIgnoreError;
use crate::{
    CalculateLspFeeResponseV2, ChannelsInfo, ExchangeRate, LightningNodeConfig, NodeInfo,
    OfferKind, RuntimeErrorCode, UserPreferences,
};
use breez_sdk_core::{
    BreezServices, OpenChannelFeeRequest, OpeningFeeParams, ReportIssueRequest,
    ReportPaymentFailureDetails, UnspentTransactionOutput,
};
use crow::OfferManager;
use honeybadger::Auth;
use log::{debug, Level};
use perro::{MapToError, OptionToError};
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
pub(crate) struct Support {
    pub user_preferences: Arc<Mutex<UserPreferences>>,
    pub sdk: Arc<BreezServices>,
    pub auth: Arc<Auth>,
    pub async_auth: Arc<honeybadger::asynchronous::Auth>,
    pub fiat_topup_client: PocketClient,
    pub offer_manager: Arc<OfferManager>,
    pub rt: Arc<AsyncRuntime>,
    pub data_store: Arc<Mutex<DataStore>>,
    pub task_manager: Arc<Mutex<TaskManager>>,
    pub allowed_countries_country_iso_3166_1_alpha_2: Vec<String>,
    pub phone_number_prefix_parser: PhoneNumberPrefixParser,
    pub persistence_encryption_key: [u8; 32],
    pub node_config: LightningNodeConfig,
    pub analytics_interceptor: Arc<AnalyticsInterceptor>,
}

impl Support {
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

    pub fn get_node_utxos(&self) -> Result<Vec<UnspentTransactionOutput>> {
        let node_state = self
            .sdk
            .node_info()
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Couldn't get node info")?;

        Ok(node_state.utxos)
    }

    /// Query the LSP fee params that the LSP offers
    /// Increased expiry dates mean higher fee rates.
    /// This method returns the best offer within the given expiry.
    ///
    /// Parameters:
    /// * `expiry` - expiry time in seconds
    ///
    /// Requires network: **yes**
    pub(crate) fn query_lsp_fee_params(&self, expiry: Option<u32>) -> Result<OpeningFeeParams> {
        let req = OpenChannelFeeRequest {
            amount_msat: None,
            expiry,
        };
        let res = self
            .rt
            .handle()
            .block_on(self.sdk.open_channel_fee(req))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to compute opening channel fee",
            )?;

        Ok(res.fee_params)
    }

    /// Calculate the actual LSP fee for the given amount of an incoming payment.
    /// If the already existing inbound capacity is enough, no new channel is required.
    /// The LSP may offer multiple fee rates, tied to different expiration dates.
    /// Increased expiry dates mean higher fee rates.
    /// This method returns the best offer within the given expiry.
    ///
    /// Parameters:
    /// * `amount_sat` - amount in sats to compute LSP fee for
    /// * `expiry` - expiry time in seconds
    ///
    /// For the returned fees to be guaranteed to be accurate, the returned `lsp_fee_params` must be
    /// provided to [`Bolt11::create`]
    ///
    /// Requires network: **yes**
    pub fn calculate_lsp_fee_for_amount(
        &self,
        amount_sat: u64,
        expiry: Option<u32>,
    ) -> Result<CalculateLspFeeResponseV2> {
        let amount_msat = Some(amount_sat.as_sats().msats);
        let req = OpenChannelFeeRequest {
            amount_msat,
            expiry,
        };
        let res = self
            .rt
            .handle()
            .block_on(self.sdk.open_channel_fee(req))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to compute opening channel fee",
            )?;
        let fee_msat = res.fee_msat.ok_or_permanent_failure(
            "Breez SDK open_channel_fee returned None lsp fee when provided with Some(amount_msat)",
        )?;
        let lsp_fee = fee_msat.as_msats().to_amount_up(&self.get_exchange_rate());

        Ok(CalculateLspFeeResponseV2 {
            lsp_fee,
            lsp_fee_params: res.fee_params,
        })
    }

    /// Calculate the actual LSP fee for the given amount of an incoming payment,
    /// providing the fee params provided by the LSP
    /// If the already existing inbound capacity is enough, no new channel is required.
    ///
    /// Parameters:
    /// * `amount_sat` - amount in sats to compute LSP fee for
    /// * `lsp_fee_param` - Fee terms offered by the LSP
    ///
    /// Requires network: **no**
    pub(crate) fn calculate_lsp_fee_for_amount_locally(
        &self,
        amount_sat: u64,
        lsp_fee_param: OpeningFeeParams,
    ) -> Result<CalculateLspFeeResponseV2> {
        // todo use Breez-SDK to do the lsp fee calculation once this is possible: https://github.com/breez/breez-sdk-greenlight/issues/1131

        let max_receivable = self
            .get_node_info()?
            .channels_info
            .max_receivable_single_payment
            .sats;
        let lsp_fee = if amount_sat > max_receivable {
            let lsp_fee_sat = amount_sat * lsp_fee_param.proportional as u64 / 1_000_000;
            let lsp_fee_msat_rounded_to_sat = lsp_fee_sat * 1000;

            std::cmp::max(lsp_fee_msat_rounded_to_sat, lsp_fee_param.min_msat)
        } else {
            0
        };

        Ok(CalculateLspFeeResponseV2 {
            lsp_fee: lsp_fee.as_msats().to_amount_up(&self.get_exchange_rate()),
            lsp_fee_params: lsp_fee_param,
        })
    }

    /// Query the current recommended on-chain fee rate.
    pub(crate) fn query_onchain_fee_rate(&self) -> Result<u32> {
        let recommended_fees = self
            .rt
            .handle()
            .block_on(self.sdk.recommended_fees())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't fetch recommended fees",
            )?;

        Ok(recommended_fees.half_hour_fee as u32)
    }

    pub fn report_send_payment_issue(&self, payment_hash: String) {
        debug!("Reporting failure of payment: {payment_hash}");
        let data = ReportPaymentFailureDetails {
            payment_hash,
            comment: None,
        };
        let request = ReportIssueRequest::PaymentFailure { data };
        self.rt
            .handle()
            .block_on(self.sdk.report_issue(request))
            .log_ignore_error(Level::Warn, "Failed to report issue");
    }

    pub fn store_payment_info(&self, hash: &str, offer: Option<OfferKind>) {
        let user_preferences = self.user_preferences.lock_unwrap().clone();
        let exchange_rates = self.get_exchange_rates();
        self.data_store
            .lock_unwrap()
            .store_payment_info(hash, user_preferences, exchange_rates, offer, None, None)
            .log_ignore_error(Level::Error, "Failed to persist payment info")
    }
}
