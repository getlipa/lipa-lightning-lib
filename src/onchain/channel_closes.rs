use crate::amount::{AsSats, Sats, ToAmount};
use crate::errors::Result;
use crate::onchain::get_onchain_resolving_fees;
use crate::support::Support;
use crate::{OnchainResolvingFees, RuntimeErrorCode, SweepInfo, CLN_DUST_LIMIT_SAT};
use breez_sdk_core::error::RedeemOnchainError;
use breez_sdk_core::PrepareRedeemOnchainFundsRequest;
use perro::{ensure, invalid_input, MapToError};
use std::sync::Arc;

pub struct ChannelCloses {
    support: Arc<Support>,
}

impl ChannelCloses {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Returns the fees for resolving channel closes if there are enough funds to pay for fees.
    ///
    /// Must only be called when there are onchain funds to resolve.
    ///
    /// Returns the fee information for the available resolving options.
    ///
    /// Requires network: **yes**
    pub fn determine_resolving_fees(&self) -> Result<Option<OnchainResolvingFees>> {
        let onchain_balance = self
            .support
            .sdk
            .node_info()
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Couldn't fetch on-chain balance",
            )?
            .onchain_balance_msat
            .as_msats();
        ensure!(
            onchain_balance.msats != 0,
            invalid_input("No on-chain funds to resolve")
        );

        let prepare_onchain_tx =
            move |to_address: String, sat_per_vbyte: u32| -> Result<(Sats, Sats)> {
                let sweep_info = self
                    .prepare_sweep(to_address, sat_per_vbyte)
                    .map_to_runtime_error(
                        RuntimeErrorCode::NodeUnavailable,
                        "Failed to prepare sweep funds from channel closes",
                    )?;

                Ok((
                    sweep_info.amount.sats.as_sats(),
                    sweep_info.onchain_fee_amount.sats.as_sats(),
                ))
            };

        get_onchain_resolving_fees(&self.support, onchain_balance, prepare_onchain_tx)
    }

    /// Prepares a sweep of all available on-chain funds to the provided on-chain address.
    ///
    /// Parameters:
    /// * `address` - the funds will be sweeped to this address
    /// * `onchain_fee_rate` - the fee rate that should be applied for the transaction.
    ///   The recommended on-chain fee rate can be queried using [`LightningNode::query_onchain_fee_rate`]
    ///
    /// Returns information on the prepared sweep, including the exact fee that results from
    /// using the provided fee rate. The method [`LightningNode::sweep_funds_from_channel_closes`] can be used to broadcast
    /// the sweep transaction.
    ///
    /// Requires network: **yes**
    pub fn prepare_sweep(
        &self,
        address: String,
        onchain_fee_rate: u32,
    ) -> std::result::Result<SweepInfo, RedeemOnchainError> {
        let res =
            self.support
                .rt
                .handle()
                .block_on(self.support.sdk.prepare_redeem_onchain_funds(
                    PrepareRedeemOnchainFundsRequest {
                        to_address: address.clone(),
                        sat_per_vbyte: onchain_fee_rate,
                    },
                ))?;

        let onchain_balance_sat = self
            .support
            .sdk
            .node_info()
            .map_err(|e| RedeemOnchainError::ServiceConnectivity {
                err: format!("Failed to fetch on-chain balance: {e}"),
            })?
            .onchain_balance_msat
            .as_msats()
            .to_amount_down(&None)
            .sats;

        let rate = self.support.get_exchange_rate();

        // Add the amount that won't be possible to be swept due to CLN's min-emergency limit (546 sats)
        // TODO: remove CLN_DUST_LIMIT_SAT addition if/when
        //      https://github.com/ElementsProject/lightning/issues/7131 is addressed
        let utxos = self
            .support
            .get_node_utxos()
            .map_err(|e| RedeemOnchainError::Generic { err: e.to_string() })?;
        let onchain_fee_sat = if utxos
            .iter()
            .any(|u| u.amount_millisatoshi == CLN_DUST_LIMIT_SAT * 1_000)
        {
            res.tx_fee_sat
        } else {
            res.tx_fee_sat + CLN_DUST_LIMIT_SAT
        };

        let onchain_fee_amount = onchain_fee_sat.as_sats().to_amount_up(&rate);

        Ok(SweepInfo {
            address,
            onchain_fee_rate,
            onchain_fee_amount,
            amount: (onchain_balance_sat - res.tx_fee_sat)
                .as_sats()
                .to_amount_up(&rate),
        })
    }
}
