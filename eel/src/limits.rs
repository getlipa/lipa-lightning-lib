const MAX_RECEIVE_AMOUNT_BETA_SAT: u64 = 1_000_000;
const MIN_RECEIVE_MULTIPLIER: u64 = 2; // Minimum receive = mutliple of setup fees

#[derive(PartialEq, Eq, Debug)]
pub struct PaymentAmountLimits {
    pub max_receive_sat: u64,
    pub liquidity_limit: LiquidityLimit,
}

#[derive(PartialEq, Eq, Debug)]
pub enum LiquidityLimit {
    None,
    MaxFreeReceive { sat_amount: u64 },
    MinReceive { sat_amount: u64 },
}

impl PaymentAmountLimits {
    pub fn fetch(inbound_capacity: u64, lsp_min_fee: u64) -> Self {
        let min_receive_amount = lsp_min_fee * MIN_RECEIVE_MULTIPLIER;

        let liquidity_limit = if inbound_capacity < min_receive_amount {
            LiquidityLimit::MinReceive {
                sat_amount: min_receive_amount,
            }
        } else if inbound_capacity < MAX_RECEIVE_AMOUNT_BETA_SAT {
            LiquidityLimit::MaxFreeReceive {
                sat_amount: inbound_capacity,
            }
        } else {
            LiquidityLimit::None
        };

        PaymentAmountLimits {
            max_receive_sat: MAX_RECEIVE_AMOUNT_BETA_SAT,
            liquidity_limit,
        }
    }
}
