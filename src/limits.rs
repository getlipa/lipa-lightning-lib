use crate::config::ReceiveLimitsConfig;
use crate::lightning::receive_limits::{LiquidityLimit, ReceiveAmountLimits};
use crate::{Amount, ExchangeRate};

/// Information on the limits imposed on the next receiving payment
pub struct PaymentAmountLimits {
    /// Hard limit: The maximum amount a user is allowed to receive per payment
    pub max_receive: Amount,
    pub liquidity_limit: LiquidityLimit,
}

impl From<ReceiveAmountLimits> for PaymentAmountLimits {
    fn from(value: ReceiveAmountLimits) -> Self {
        Self {
            max_receive: value.max_receive,
            liquidity_limit: value.liquidity_limit,
        }
    }
}

impl PaymentAmountLimits {
    pub fn calculate(
        inbound_capacity_sat: u64,
        lsp_min_fee_sat: u64,
        exchange_rate: &Option<ExchangeRate>,
        receive_limits_config: &ReceiveLimitsConfig,
    ) -> Self {
        ReceiveAmountLimits::calculate(
            inbound_capacity_sat,
            lsp_min_fee_sat,
            exchange_rate,
            receive_limits_config,
        )
        .into()
    }
}
