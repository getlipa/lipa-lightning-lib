use crate::amount::{AsSats, Permyriad, ToAmount};
use crate::errors::Result;
use crate::support::Support;
use crate::{ClearWalletInfo, RangeHit, RuntimeErrorCode};
use breez_sdk_core::{BitcoinAddressData, PayOnchainRequest, PrepareOnchainPaymentRequest};
use perro::{permanent_failure, MapToError};
use std::sync::Arc;

pub struct ReverseSwap {
    support: Arc<Support>,
}

impl ReverseSwap {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Check if clearing the wallet is feasible.
    ///
    /// Meaning that the balance is within the range of what can be reverse-swapped.
    ///
    /// Requires network: **yes**
    pub fn determine_clear_wallet_feasibility(&self) -> Result<RangeHit> {
        let limits = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.onchain_payment_limits())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get on-chain payment limits",
            )?;
        let balance_sat = self
            .support
            .sdk
            .node_info()
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to read node info",
            )?
            .channels_balance_msat
            .as_msats()
            .sats_round_down()
            .sats;
        let exchange_rate = self.support.get_exchange_rate();

        // Accomodating lightning network routing fees.
        let routing_fee = Permyriad(
            self.support
                .node_config
                .max_routing_fee_config
                .max_routing_fee_permyriad,
        )
        .of(&limits.min_sat.as_sats())
        .sats_round_up();
        let min = limits.min_sat + routing_fee.sats;
        let range_hit = match balance_sat {
            balance_sat if balance_sat < min => RangeHit::Below {
                min: min.as_sats().to_amount_up(&exchange_rate),
            },
            balance_sat if balance_sat <= limits.max_sat => RangeHit::In,
            balance_sat if limits.max_sat < balance_sat => RangeHit::Above {
                max: limits.max_sat.as_sats().to_amount_down(&exchange_rate),
            },
            _ => permanent_failure!("Unreachable code in check_clear_wallet_feasibility()"),
        };
        Ok(range_hit)
    }

    /// Prepares a reverse swap that sends all funds in LN channels. This is possible because the
    /// route to the swap service is known, so fees can be known in advance.
    ///
    /// This can fail if the balance is either too low or too high for it to be reverse-swapped.
    /// The method [`ReverseSwap::determine_clear_wallet_feasibility`] can be used to check if the balance
    /// is within the required range.
    ///
    /// Requires network: **yes**
    pub fn prepare_clear_wallet(&self) -> Result<ClearWalletInfo> {
        let claim_tx_feerate = self.support.query_onchain_fee_rate()?;
        let limits = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.onchain_payment_limits())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get on-chain payment limits",
            )?;
        let prepare_response = self
            .support
            .rt
            .handle()
            .block_on(
                self.support
                    .sdk
                    .prepare_onchain_payment(PrepareOnchainPaymentRequest {
                        amount_sat: limits.max_payable_sat,
                        amount_type: breez_sdk_core::SwapAmountType::Send,
                        claim_tx_feerate,
                    }),
            )
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to prepare on-chain payment",
            )?;

        let total_fees_sat = prepare_response.total_fees;
        let onchain_fee_sat = prepare_response.fees_claim + prepare_response.fees_lockup;
        let swap_fee_sat = total_fees_sat - onchain_fee_sat;
        let exchange_rate = self.support.get_exchange_rate();

        Ok(ClearWalletInfo {
            clear_amount: prepare_response
                .sender_amount_sat
                .as_sats()
                .to_amount_up(&exchange_rate),
            total_estimated_fees: total_fees_sat.as_sats().to_amount_up(&exchange_rate),
            onchain_fee: onchain_fee_sat.as_sats().to_amount_up(&exchange_rate),
            swap_fee: swap_fee_sat.as_sats().to_amount_up(&exchange_rate),
            prepare_response,
        })
    }

    /// Starts a reverse swap that sends all funds in LN channels to the provided on-chain address.
    ///
    /// Parameters:
    /// * `clear_wallet_info` - An instance of [`ClearWalletInfo`] obtained using
    ///   [`ReverseSwap::prepare_clear_wallet`].
    /// * `destination_onchain_address_data` - An on-chain address data instance. Can be obtained
    ///   using [`LightningNode::decode_data`](crate::LightningNode::decode_data).
    ///
    /// Requires network: **yes**
    pub fn clear_wallet(
        &self,
        clear_wallet_info: ClearWalletInfo,
        destination_onchain_address_data: BitcoinAddressData,
    ) -> Result<()> {
        self.support
            .rt
            .handle()
            .block_on(self.support.sdk.pay_onchain(PayOnchainRequest {
                recipient_address: destination_onchain_address_data.address,
                prepare_res: clear_wallet_info.prepare_response,
            }))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to start reverse swap",
            )?;
        Ok(())
    }
}
