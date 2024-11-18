use crate::amount::{AsSats, ToAmount};
use crate::node_config::ReceiveLimitsConfig;
use crate::{Amount, ExchangeRate};

/// Information on the limits imposed on the next receiving payment
pub struct ReceiveAmountLimits {
    /// Hard limit: The maximum amount a user is allowed to receive per payment
    pub max_receive: Amount,
    pub liquidity_limit: LiquidityLimit,
}

pub enum LiquidityLimit {
    /// inbound capacity >= max_receive
    None,
    /// Soft limit: The maximum amount a user can receive without being charged a setup fee
    MaxFreeReceive { amount: Amount },
    /// Hard limit: The minimum amount a user must receive with the next payment
    /// If this limit is provided, that means a setup fee will be charged for the incoming payment
    MinReceive { amount: Amount },
}

impl ReceiveAmountLimits {
    pub fn calculate(
        inbound_capacity_sat: u64,
        lsp_min_fee_sat: u64,
        exchange_rate: &Option<ExchangeRate>,
        receive_limits_config: &ReceiveLimitsConfig,
    ) -> Self {
        let min_receive_sat = (lsp_min_fee_sat as f64
            * receive_limits_config.min_receive_channel_open_fee_multiplier)
            as u64;

        let liquidity_limit = if inbound_capacity_sat < min_receive_sat {
            LiquidityLimit::MinReceive {
                amount: min_receive_sat.as_sats().to_amount_up(exchange_rate),
            }
        } else if inbound_capacity_sat < receive_limits_config.max_receive_amount_sat {
            LiquidityLimit::MaxFreeReceive {
                amount: inbound_capacity_sat.as_sats().to_amount_down(exchange_rate),
            }
        } else {
            LiquidityLimit::None
        };

        ReceiveAmountLimits {
            max_receive: receive_limits_config
                .max_receive_amount_sat
                .as_sats()
                .to_amount_down(exchange_rate),
            liquidity_limit,
        }
    }
}
