use crate::amount::ToAmount;
use crate::{Amount, ExchangeRate};

const MAX_RECEIVE_AMOUNT_BETA_SAT: u64 = 1_000_000;
const MIN_RECEIVE_MULTIPLIER: u64 = 2; // Minimum receive = mutliple of setup fees

pub struct PaymentAmountLimits {
    pub max_receive: Amount,
    pub liquidity_limit: LiquidityLimit,
}

pub enum LiquidityLimit {
    None,
    MaxFreeReceive { amount: Amount },
    MinReceive { amount: Amount },
}

impl PaymentAmountLimits {
    pub fn calculate(
        inbound_capacity_sat: u64,
        lsp_min_fee_sat: u64,
        exchange_rate: &Option<ExchangeRate>,
    ) -> Self {
        let min_receive_sat = lsp_min_fee_sat * MIN_RECEIVE_MULTIPLIER;

        let liquidity_limit = if inbound_capacity_sat < min_receive_sat {
            LiquidityLimit::MinReceive {
                amount: (min_receive_sat * 1000).to_amount_up(exchange_rate),
            }
        } else if inbound_capacity_sat < MAX_RECEIVE_AMOUNT_BETA_SAT {
            LiquidityLimit::MaxFreeReceive {
                amount: (inbound_capacity_sat * 1000).to_amount_down(exchange_rate),
            }
        } else {
            LiquidityLimit::None
        };

        PaymentAmountLimits {
            max_receive: (MAX_RECEIVE_AMOUNT_BETA_SAT * 1000).to_amount_down(exchange_rate),
            liquidity_limit,
        }
    }
}
