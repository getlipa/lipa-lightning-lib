pub mod bolt11;
pub mod lnurl;
pub mod receive_limits;

use crate::amount::{AsSats, Permyriad, ToAmount};
use crate::errors::Result;
use crate::lightning::bolt11::Bolt11;
use crate::lightning::lnurl::Lnurl;
use crate::lightning::receive_limits::ReceiveAmountLimits;
use crate::locker::Locker;
use crate::support::Support;
use crate::{
    CalculateLspFeeResponse, ExchangeRate, LspFee, MaxRoutingFeeConfig, MaxRoutingFeeMode,
    RuntimeErrorCode,
};
use perro::MapToError;
use std::sync::Arc;

/// Payment affordability returned by [`Lightning::determine_payment_affordability`].
#[derive(Debug)]
pub enum PaymentAffordability {
    /// Not enough funds available to pay the requested amount.
    NotEnoughFunds,
    /// Not enough funds available to pay the requested amount and the max routing fees.
    /// There might be a route that is affordable enough but it is unknown until tried.
    UnaffordableFees,
    /// Enough funds for the payment and routing fees are available.
    Affordable,
}

pub struct Lightning {
    bolt11: Arc<Bolt11>,
    lnurl: Arc<Lnurl>,
    support: Arc<Support>,
}

impl Lightning {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(support: Arc<Support>) -> Self {
        let bolt11 = Arc::new(Bolt11::new(Arc::clone(&support)));
        let lnurl = Arc::new(Lnurl::new(Arc::clone(&support)));
        Self {
            bolt11,
            lnurl,
            support,
        }
    }

    pub fn bolt11(&self) -> Arc<Bolt11> {
        Arc::clone(&self.bolt11)
    }

    pub fn lnurl(&self) -> Arc<Lnurl> {
        Arc::clone(&self.lnurl)
    }

    /// Determine the max routing fee mode that will be employed to restrict the fees for paying a
    /// given amount in sats.
    ///
    /// Requires network: **no**
    pub fn determine_max_routing_fee_mode(&self, amount_sat: u64) -> MaxRoutingFeeMode {
        get_payment_max_routing_fee_mode(
            &self.support.node_config.max_routing_fee_config,
            amount_sat,
            &self.support.get_exchange_rate(),
        )
    }

    /// Checks if the given amount could be spent on a payment.
    ///
    /// Parameters:
    /// * `amount` - The to be spent amount.
    ///
    /// Requires network: **no**
    pub fn determine_payment_affordability(
        &self,
        amount_sat: u64,
    ) -> crate::Result<PaymentAffordability> {
        let amount = amount_sat.as_sats();

        let routing_fee_mode = self.determine_max_routing_fee_mode(amount_sat);

        let max_fee_msats = match routing_fee_mode {
            MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
                Permyriad(max_fee_permyriad).of(&amount).msats
            }
            MaxRoutingFeeMode::Absolute { max_fee_amount } => max_fee_amount.sats.as_sats().msats,
        };

        let node_state = self.support.sdk.node_info().map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to read node info",
        )?;

        if amount.msats > node_state.max_payable_msat {
            return Ok(PaymentAffordability::NotEnoughFunds);
        }

        if amount.msats + max_fee_msats > node_state.max_payable_msat {
            return Ok(PaymentAffordability::UnaffordableFees);
        }

        Ok(PaymentAffordability::Affordable)
    }

    /// Get the current limits for the amount that can be transferred in a single payment.
    /// Currently there are only limits for receiving payments.
    /// The limits (partly) depend on the channel situation of the node, so it should be called
    /// again every time the user is about to receive a payment.
    /// The limits stay the same regardless of what amount wants to receive (= no changes while
    /// he's typing the amount)
    ///
    /// Requires network: **no**
    pub fn determine_receive_amount_limits(&self) -> Result<ReceiveAmountLimits> {
        // TODO: try to move this logic inside the SDK
        let lsp_min_fee_amount = self.get_lsp_fee()?.channel_minimum_fee;
        let max_inbound_amount = self
            .support
            .get_node_info()?
            .channels_info
            .total_inbound_capacity;
        Ok(ReceiveAmountLimits::calculate(
            max_inbound_amount.sats,
            lsp_min_fee_amount.sats,
            &self.support.get_exchange_rate(),
            &self.support.node_config.receive_limits_config,
        ))
    }

    /// Calculate the actual LSP fee for the given amount of an incoming payment.
    /// If the already existing inbound capacity is enough, no new channel is required.
    ///
    /// Parameters:
    /// * `amount_sat` - amount in sats to compute LSP fee for
    ///
    /// For the returned fees to be guaranteed to be accurate, the returned `lsp_fee_params` must be
    /// provided to [`Bolt11::create`]
    ///
    /// For swaps, use [`Swap::calculate_lsp_fee_for_amount`] instead,
    /// which uses fee offer from the LSP that is valid for a longer time period
    ///
    /// Requires network: **yes**
    pub fn calculate_lsp_fee_for_amount(&self, amount_sat: u64) -> Result<CalculateLspFeeResponse> {
        self.support.calculate_lsp_fee_for_amount(amount_sat, None)
    }

    /// When *receiving* payments, a new channel MAY be required. A fee will be charged to the user.
    /// This does NOT impact *sending* payments.
    /// Get information about the fee charged by the LSP for opening new channels
    ///
    /// Requires network: **no**
    pub fn get_lsp_fee(&self) -> Result<LspFee> {
        let exchange_rate = self.support.get_exchange_rate();
        let lsp_fee = self.support.task_manager.lock_unwrap().get_lsp_fee()?;
        Ok(LspFee {
            channel_minimum_fee: lsp_fee.min_msat.as_msats().to_amount_up(&exchange_rate),
            channel_fee_permyriad: lsp_fee.proportional as u64 / 100,
        })
    }
}

fn get_payment_max_routing_fee_mode(
    config: &MaxRoutingFeeConfig,
    amount_sat: u64,
    exchange_rate: &Option<ExchangeRate>,
) -> MaxRoutingFeeMode {
    let max_fee_permyriad = Permyriad(config.max_routing_fee_permyriad);
    let relative_fee = max_fee_permyriad.of(&amount_sat.as_sats());
    if relative_fee.msats < config.max_routing_fee_exempt_fee_sats.as_sats().msats {
        MaxRoutingFeeMode::Absolute {
            max_fee_amount: config
                .max_routing_fee_exempt_fee_sats
                .as_sats()
                .to_amount_up(exchange_rate),
        }
    } else {
        MaxRoutingFeeMode::Relative {
            max_fee_permyriad: max_fee_permyriad.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::amount::{Permyriad, Sats};
    use crate::lightning::get_payment_max_routing_fee_mode;
    use crate::{MaxRoutingFeeConfig, MaxRoutingFeeMode};

    const MAX_FEE_PERMYRIAD: Permyriad = Permyriad(150);
    const EXEMPT_FEE: Sats = Sats::new(21);

    #[test]
    fn test_get_payment_max_routing_fee_mode_absolute() {
        let max_routing_mode = get_payment_max_routing_fee_mode(
            &MaxRoutingFeeConfig {
                max_routing_fee_permyriad: MAX_FEE_PERMYRIAD.0,
                max_routing_fee_exempt_fee_sats: EXEMPT_FEE.sats,
            },
            EXEMPT_FEE.msats / ((MAX_FEE_PERMYRIAD.0 as u64) / 10) - 1,
            &None,
        );

        match max_routing_mode {
            MaxRoutingFeeMode::Absolute { max_fee_amount } => {
                assert_eq!(max_fee_amount.sats, EXEMPT_FEE.sats);
            }
            _ => {
                panic!("Unexpected variant");
            }
        }
    }

    #[test]
    fn test_get_payment_max_routing_fee_mode_relative() {
        let max_routing_mode = get_payment_max_routing_fee_mode(
            &MaxRoutingFeeConfig {
                max_routing_fee_permyriad: MAX_FEE_PERMYRIAD.0,
                max_routing_fee_exempt_fee_sats: EXEMPT_FEE.sats,
            },
            EXEMPT_FEE.msats / ((MAX_FEE_PERMYRIAD.0 as u64) / 10),
            &None,
        );

        match max_routing_mode {
            MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
                assert_eq!(max_fee_permyriad, MAX_FEE_PERMYRIAD.0);
            }
            _ => {
                panic!("Unexpected variant");
            }
        }
    }
}
