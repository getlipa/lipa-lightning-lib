use crate::amount::{AsSats, Sats, ToAmount};
use crate::errors::Result;
use crate::onchain::{get_onchain_resolving_fees, query_onchain_fee_rate};
use crate::support::Support;
use crate::util::unix_timestamp_to_system_time;
use crate::{
    Amount, CalculateLspFeeResponseV2, FailedSwapInfo, OnchainResolvingFees, ResolveFailedSwapInfo,
    RuntimeErrorCode, SwapAddressInfo,
};
use breez_sdk_core::error::ReceiveOnchainError;
use breez_sdk_core::{
    BitcoinAddressData, Network, OpeningFeeParams, PrepareRefundRequest, ReceiveOnchainRequest,
    RefundRequest,
};
use perro::{ensure, runtime_error, MapToError};
use std::sync::Arc;

const TWO_WEEKS: u32 = 2 * 7 * 24 * 60 * 60;

pub struct Swap {
    support: Arc<Support>,
}

impl Swap {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Generates a Bitcoin on-chain address that can be used to topup the local LN wallet from an
    /// external on-chain wallet.
    ///
    /// Funds sent to this address should conform to the min and max values provided within
    /// [`SwapAddressInfo`].
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// Parameters:
    /// * `lsp_fee_params` - the lsp fee parameters to be used if a new channel needs to
    ///   be opened. Can be obtained using [`LightningNode::calculate_lsp_fee`](crate::LightningNode::calculate_lsp_fee).
    ///
    /// Requires network: **yes**
    pub fn create(
        &self,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> std::result::Result<SwapAddressInfo, ReceiveOnchainError> {
        let swap_info = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.receive_onchain(ReceiveOnchainRequest {
                opening_fee_params: lsp_fee_params,
            }))?;
        let rate = self.support.get_exchange_rate();

        Ok(SwapAddressInfo {
            address: swap_info.bitcoin_address,
            min_deposit: (swap_info.min_allowed_deposit as u64)
                .as_sats()
                .to_amount_up(&rate),
            max_deposit: (swap_info.max_allowed_deposit as u64)
                .as_sats()
                .to_amount_down(&rate),
            swap_fee: 0_u64.as_sats().to_amount_up(&rate),
        })
    }

    /// Returns the fees for resolving a failed swap if there are enough funds to pay for fees.
    ///
    /// Must only be called when the failed swap is unresolved.
    ///
    /// Returns the fee information for the available resolving options.
    ///
    /// Requires network: *yes*
    pub fn determine_resolving_fees(
        &self,
        failed_swap_info: FailedSwapInfo,
    ) -> Result<Option<OnchainResolvingFees>> {
        let failed_swap_closure = failed_swap_info.clone();
        let prepare_onchain_tx = move |address: String| -> Result<(Sats, Sats, u32)> {
            let sweep_info = self.prepare_sweep(
                failed_swap_closure,
                BitcoinAddressData {
                    address,
                    network: Network::Bitcoin,
                    amount_sat: None,
                    label: None,
                    message: None,
                },
            )?;

            Ok((
                sweep_info.recovered_amount.sats.as_sats(),
                sweep_info.onchain_fee.sats.as_sats(),
                sweep_info.onchain_fee_rate,
            ))
        };
        get_onchain_resolving_fees(
            &self.support,
            self,
            failed_swap_info.amount.sats.as_sats().msats(),
            prepare_onchain_tx,
        )
    }

    /// Prepares the sweep transaction for failed swap in order to know how much will be recovered
    /// and how much will be paid in on-chain fees.
    ///
    /// Parameters:
    /// * `failed_swap_info` - the failed swap that will be prepared
    /// * `destination` - the destination address to which funds will be sent.
    ///     Can be obtained using [`Util::decode_data`](crate::Util::decode_data)
    ///
    /// Requires network: **yes**
    pub fn prepare_sweep(
        &self,
        failed_swap_info: FailedSwapInfo,
        destination: BitcoinAddressData,
    ) -> Result<SweepFailedSwapInfo> {
        let to_address = destination.address;
        let onchain_fee_rate = query_onchain_fee_rate(&self.support)?;
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

        Ok(SweepFailedSwapInfo {
            swap_address: failed_swap_info.address,
            recovered_amount,
            onchain_fee,
            to_address,
            onchain_fee_rate,
        })
    }

    /// Creates and broadcasts a sweeping transaction to recover funds from a failed swap. Existing
    /// failed swaps can be listed using [`ActionsRequired::list`](crate::ActionsRequired::list) and preparing
    /// the resolution of a failed swap can be done using [`Swap::prepare_sweep`].
    ///
    /// Parameters:
    /// * `sweep_failed_swap_info` - Information needed to sweep the failed swap. Can be obtained
    ///   using [`Swap::prepare_sweep`].
    ///
    /// Returns the txid of the resolving transaction.
    ///
    /// Paid on-chain fees can be known in advance using [`Swap::prepare_sweep`].
    ///
    /// Requires network: **yes**
    pub fn sweep(&self, sweep_failed_swap_info: SweepFailedSwapInfo) -> Result<String> {
        Ok(self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.refund(RefundRequest {
                swap_address: sweep_failed_swap_info.swap_address,
                to_address: sweep_failed_swap_info.to_address,
                sat_per_vbyte: sweep_failed_swap_info.onchain_fee_rate,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to create and broadcast failed swap refund transaction",
            )?
            .refund_tx_id)
    }

    /// Automatically swaps failed swap funds back to lightning.
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// If the current balance doesn't fulfill the limits, this method will return an error.
    /// Before using this method use [`Swap::determine_resolving_fees`] to validate a swap is available.
    ///
    /// Parameters:
    /// * `sat_per_vbyte` - the fee rate to use for the on-chain transaction.
    ///   Can be obtained with [`Swap::determine_resolving_fees`].
    /// * `lsp_fee_params` - the lsp fee params for opening a new channel if necessary.
    ///   Can be obtained with [`Swap::determine_resolving_fees`].
    ///
    /// Returns the txid of the sweeping tx.
    ///
    /// Requires network: **yes**
    pub fn swap(
        &self,
        failed_swap_info: FailedSwapInfo,
        sats_per_vbyte: u32,
        lsp_fee_param: Option<OpeningFeeParams>,
    ) -> Result<String> {
        let lsp_fee_param =
            lsp_fee_param.unwrap_or(self.support.query_lsp_fee_params(Some(TWO_WEEKS))?);

        let swap_address_info = self
            .create(Some(lsp_fee_param.clone()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't generate swap address",
            )?;

        let prepare_response = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.prepare_refund(PrepareRefundRequest {
                swap_address: failed_swap_info.address.clone(),
                to_address: swap_address_info.address.clone(),
                sat_per_vbyte: sats_per_vbyte,
            }))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Coudln't prepare refund")?;

        let send_amount_sats = failed_swap_info.amount.sats - prepare_response.refund_tx_fee_sat;

        ensure!(
            swap_address_info.min_deposit.sats <= send_amount_sats,
            runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed swap amount isn't enough for creating new swap"
            )
        );

        ensure!(
            swap_address_info.max_deposit.sats >= send_amount_sats,
            runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed swap amount is too big for creating new swap"
            )
        );

        let lsp_fees = self
            .support
            .calculate_lsp_fee_for_amount_locally(send_amount_sats, lsp_fee_param)?
            .lsp_fee
            .sats;

        ensure!(
            lsp_fees < send_amount_sats,
            runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "A new channel is needed and the failed swap amount is not enough to pay for fees"
            )
        );

        let refund_response = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.refund(RefundRequest {
                swap_address: failed_swap_info.address,
                to_address: swap_address_info.address,
                sat_per_vbyte: sats_per_vbyte,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't broadcast swap refund transaction",
            )?;

        Ok(refund_response.refund_tx_id)
    }

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

    /// Calculate the actual LSP fee for the given amount of a swap.
    /// If the already existing inbound capacity is enough, no new channel is required.
    ///
    /// Parameters:
    /// * `amount_sat` - amount in sats to compute LSP fee for
    ///
    /// Requires network: **yes**
    pub fn calculate_lsp_fee_for_amount(
        &self,
        amount_sat: u64,
    ) -> Result<CalculateLspFeeResponseV2> {
        self.support
            .calculate_lsp_fee_for_amount(amount_sat, Some(TWO_WEEKS))
    }
}

/// Information the resolution of a failed swap.
pub struct SweepFailedSwapInfo {
    /// The address of the failed swap.
    pub swap_address: String,
    /// The amount that will be sent (swap amount - on-chain fee).
    pub recovered_amount: Amount,
    /// The amount that will be paid in on-chain fees.
    pub onchain_fee: Amount,
    /// The address to which recovered funds will be sent.
    pub to_address: String,
    /// The on-chain fee rate that will be applied. This fee rate results in the `onchain_fee`.
    pub onchain_fee_rate: u32,
}

impl From<ResolveFailedSwapInfo> for SweepFailedSwapInfo {
    fn from(value: ResolveFailedSwapInfo) -> Self {
        Self {
            swap_address: value.swap_address,
            recovered_amount: value.recovered_amount,
            onchain_fee: value.onchain_fee,
            to_address: value.to_address,
            onchain_fee_rate: value.onchain_fee_rate,
        }
    }
}

impl From<SweepFailedSwapInfo> for ResolveFailedSwapInfo {
    fn from(value: SweepFailedSwapInfo) -> Self {
        Self {
            swap_address: value.swap_address,
            recovered_amount: value.recovered_amount,
            onchain_fee: value.onchain_fee,
            to_address: value.to_address,
            onchain_fee_rate: value.onchain_fee_rate,
        }
    }
}
