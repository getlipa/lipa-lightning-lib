use crate::amount::{AsSats, ToAmount};
use crate::errors::Result;
use crate::support::Support;
use crate::util::unix_timestamp_to_system_time;
use crate::{FailedSwapInfo, ResolveFailedSwapInfo, RuntimeErrorCode};
use breez_sdk_core::PrepareRefundRequest;
use perro::MapToError;
use std::sync::Arc;

pub struct Swaps {
    support: Arc<Support>,
}

impl Swaps {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Lists all unresolved failed swaps. Each individual failed swap can be recovered
    /// using [`LightningNode::resolve_failed_swap`].
    ///
    /// Requires network: **yes**
    pub(crate) fn list_failed_unresolved(&self) -> Result<Vec<FailedSwapInfo>> {
        Ok(self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.list_refundables())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to list refundable failed swaps",
            )?
            .into_iter()
            .filter(|s| s.refund_tx_ids.is_empty())
            .map(|s| FailedSwapInfo {
                address: s.bitcoin_address,
                amount: s
                    .confirmed_sats
                    .as_sats()
                    .to_amount_down(&self.support.get_exchange_rate()),
                created_at: unix_timestamp_to_system_time(s.created_at as u64),
            })
            .collect())
    }

    /// Prepares the sweep transaction for failed swap in order to know how much will be recovered
    /// and how much will be paid in on-chain fees.
    ///
    /// Parameters:
    /// * `failed_swap_info` - the failed swap that will be prepared
    /// * `to_address` - the destination address to which funds will be sent
    /// * `onchain_fee_rate` - the fee rate that will be applied. The recommended one can be fetched
    ///   using [`LightningNode::query_onchain_fee_rate`]
    ///
    /// Requires network: **yes**
    pub fn prepare_sweep(
        &self,
        failed_swap_info: FailedSwapInfo,
        to_address: String,
        onchain_fee_rate: u32,
    ) -> Result<ResolveFailedSwapInfo> {
        let response = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.prepare_refund(PrepareRefundRequest {
                swap_address: failed_swap_info.address.clone(),
                to_address: to_address.clone(),
                sat_per_vbyte: onchain_fee_rate,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to prepare a failed swap refund transaction",
            )?;

        let rate = self.support.get_exchange_rate();
        let onchain_fee = response.refund_tx_fee_sat.as_sats().to_amount_up(&rate);
        let recovered_amount = (failed_swap_info.amount.sats - onchain_fee.sats)
            .as_sats()
            .to_amount_down(&rate);

        Ok(ResolveFailedSwapInfo {
            swap_address: failed_swap_info.address,
            recovered_amount,
            onchain_fee,
            to_address,
            onchain_fee_rate,
        })
    }
}
