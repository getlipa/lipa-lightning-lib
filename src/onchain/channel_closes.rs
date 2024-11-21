use crate::amount::{AsSats, Sats, ToAmount};
use crate::errors::Result;
use crate::onchain::swap::Swap;
use crate::onchain::{get_onchain_resolving_fees, query_onchain_fee_rate};
use crate::support::Support;
use crate::{Amount, OnchainResolvingFees, RuntimeErrorCode, SweepInfo, CLN_DUST_LIMIT_SAT};
use breez_sdk_core::error::RedeemOnchainError;
use breez_sdk_core::{
    BitcoinAddressData, Network, OpeningFeeParams, PrepareRedeemOnchainFundsRequest,
    RedeemOnchainFundsRequest,
};
use perro::{ensure, invalid_input, MapToError};
use std::sync::Arc;

pub struct ChannelClose {
    support: Arc<Support>,
    swap: Arc<Swap>,
}

impl ChannelClose {
    pub(crate) fn new(support: Arc<Support>, swap: Arc<Swap>) -> Self {
        Self { support, swap }
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

        let prepare_onchain_tx = move |address: String| -> Result<(Sats, Sats, u32)> {
            let sweep_info = self
                .prepare_sweep(BitcoinAddressData {
                    address,
                    network: Network::Bitcoin,
                    amount_sat: None,
                    label: None,
                    message: None,
                })
                .map_to_runtime_error(
                    RuntimeErrorCode::NodeUnavailable,
                    "Failed to prepare sweep funds from channel closes",
                )?;

            Ok((
                sweep_info.amount.sats.as_sats(),
                sweep_info.onchain_fee_amount.sats.as_sats(),
                sweep_info.onchain_fee_rate,
            ))
        };

        get_onchain_resolving_fees(&self.support, onchain_balance, prepare_onchain_tx)
    }

    /// Prepares a sweep of all available on-chain funds to the provided on-chain address.
    ///
    /// Parameters:
    /// * `destination` - the destination address to which funds will be sent.
    ///     Can be obtained using [`Util::decode_data`](crate::Util::decode_data)
    ///
    /// Returns information on the prepared sweep, including the exact fee that results from
    /// using the provided fee rate. The method [`ChannelClose::sweep`] can be used to broadcast
    /// the sweep transaction.
    ///
    /// Requires network: **yes**
    pub fn prepare_sweep(
        &self,
        destination: BitcoinAddressData,
    ) -> std::result::Result<SweepChannelCloseInfo, RedeemOnchainError> {
        let address = destination.address;
        let onchain_fee_rate = query_onchain_fee_rate(&self.support)
            .map_err(|e| RedeemOnchainError::ServiceConnectivity { err: e.to_string() })?;
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

        Ok(SweepChannelCloseInfo {
            address,
            onchain_fee_rate,
            onchain_fee_amount,
            amount: (onchain_balance_sat - res.tx_fee_sat)
                .as_sats()
                .to_amount_up(&rate),
        })
    }

    /// Sweeps all available channel close funds to the specified on-chain address.
    ///
    /// Parameters:
    /// * `sweep_info` - a prepared sweep info that can be obtained using
    ///     [`ChannelClose::prepare_sweep`]
    ///
    /// Returns the txid of the sweep transaction.
    ///
    /// Requires network: **yes**
    pub fn sweep(&self, sweep_info: SweepChannelCloseInfo) -> Result<String> {
        let txid = self
            .support
            .rt
            .handle()
            .block_on(
                self.support
                    .sdk
                    .redeem_onchain_funds(RedeemOnchainFundsRequest {
                        to_address: sweep_info.address,
                        sat_per_vbyte: sweep_info.onchain_fee_rate,
                    }),
            )
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to sweep funds")?
            .txid;
        Ok(hex::encode(txid))
    }

    /// Automatically swaps on-chain funds back to lightning.
    ///
    /// If a swap is in progress, this method will return an error.
    ///
    /// If the current balance doesn't fulfill the limits, this method will return an error.
    /// Before using this method use [`ChannelClose::determine_resolving_fees`] to validate a swap is available.
    ///
    /// Parameters:
    /// * `sat_per_vbyte` - the fee rate to use for the on-chain transaction.
    ///   Can be obtained with [`ChannelClose::determine_resolving_fees`].
    /// * `lsp_fee_params` - the lsp fee params for opening a new channel if necessary.
    ///   Can be obtained with [`ChannelClose::determine_resolving_fees`].
    ///
    /// Returns the txid of the sweeping tx.
    ///
    /// Requires network: **yes**
    pub fn swap(
        &self,
        sat_per_vbyte: u32,
        lsp_fee_params: Option<OpeningFeeParams>,
    ) -> std::result::Result<String, RedeemOnchainError> {
        let onchain_balance = self
            .support
            .sdk
            .node_info()?
            .onchain_balance_msat
            .as_msats();

        let swap_address_info =
            self.swap
                .create(lsp_fee_params.clone())
                .map_err(|e| RedeemOnchainError::Generic {
                    err: format!("Couldn't generate swap address: {}", e),
                })?;

        let prepare_response =
            self.support
                .rt
                .handle()
                .block_on(self.support.sdk.prepare_redeem_onchain_funds(
                    PrepareRedeemOnchainFundsRequest {
                        to_address: swap_address_info.address.clone(),
                        sat_per_vbyte,
                    },
                ))?;
        // TODO: remove CLN_DUST_LIMIT_SAT component if/when
        //      https://github.com/ElementsProject/lightning/issues/7131 is addressed
        let send_amount_sats = onchain_balance.sats_round_down().sats
            - CLN_DUST_LIMIT_SAT
            - prepare_response.tx_fee_sat;

        if swap_address_info.min_deposit.sats > send_amount_sats {
            return Err(RedeemOnchainError::InsufficientFunds {
                err: format!(
                    "Not enough funds ({} sats after onchain fees) available for min swap amount({} sats)",
                    send_amount_sats,
                    swap_address_info.min_deposit.sats,
                ),
            });
        }

        if swap_address_info.max_deposit.sats < send_amount_sats {
            return Err(RedeemOnchainError::Generic {
                err: format!(
                    "Available funds ({} sats after onchain fees) exceed limit for swap ({} sats)",
                    send_amount_sats, swap_address_info.max_deposit.sats,
                ),
            });
        }

        let lsp_fees = self
            .support
            .calculate_lsp_fee_for_amount(send_amount_sats)
            .map_err(|_| RedeemOnchainError::ServiceConnectivity {
                err: "Could not get lsp fees".to_string(),
            })?
            .lsp_fee
            .sats;
        if lsp_fees >= send_amount_sats {
            return Err(RedeemOnchainError::InsufficientFunds {
                err: format!(
                    "Available funds ({} sats after onchain fees) are not enough for lsp fees ({} sats)",
                    send_amount_sats, lsp_fees,
                ),
            });
        }

        let sweep_result =
            self.support
                .rt
                .handle()
                .block_on(
                    self.support
                        .sdk
                        .redeem_onchain_funds(RedeemOnchainFundsRequest {
                            to_address: swap_address_info.address,
                            sat_per_vbyte,
                        }),
                )?;

        Ok(hex::encode(sweep_result.txid))
    }
}

#[derive(Clone)]
pub struct SweepChannelCloseInfo {
    pub address: String,
    pub onchain_fee_rate: u32,
    pub onchain_fee_amount: Amount,
    pub amount: Amount,
}

impl From<SweepInfo> for SweepChannelCloseInfo {
    fn from(value: SweepInfo) -> Self {
        Self {
            address: value.address,
            onchain_fee_rate: value.onchain_fee_rate,
            onchain_fee_amount: value.onchain_fee_amount,
            amount: value.amount,
        }
    }
}

impl From<SweepChannelCloseInfo> for SweepInfo {
    fn from(value: SweepChannelCloseInfo) -> Self {
        Self {
            address: value.address,
            onchain_fee_rate: value.onchain_fee_rate,
            onchain_fee_amount: value.onchain_fee_amount,
            amount: value.amount,
        }
    }
}
