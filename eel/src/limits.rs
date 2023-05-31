const MAX_RECEIVE_AMOUNT_BETA_MSAT: u64 = 1_000_000_000;
const MIN_RECEIVE_MULTIPLIER: u64 = 2; // Minimum receive = mutliple of setup fees

#[derive(PartialEq, Eq, Debug)]
pub struct PaymentAmountLimits {
    pub max_receive_msat: u64,
    pub liquidity_limit: LiquidityLimit,
}

#[derive(PartialEq, Eq, Debug)]
pub enum LiquidityLimit {
    None,
    MaxFreeReceive { amount_msat: u64 },
    MinReceive { amount_msat: u64 },
}

impl PaymentAmountLimits {
    pub fn calculate(inbound_capacity_msat: u64, lsp_min_fee: u64) -> Self {
        let min_receive_msat = lsp_min_fee * MIN_RECEIVE_MULTIPLIER;

        let liquidity_limit = if inbound_capacity_msat < min_receive_msat {
            LiquidityLimit::MinReceive {
                amount_msat: min_receive_msat,
            }
        } else if inbound_capacity_msat < MAX_RECEIVE_AMOUNT_BETA_MSAT {
            LiquidityLimit::MaxFreeReceive {
                amount_msat: inbound_capacity_msat,
            }
        } else {
            LiquidityLimit::None
        };

        PaymentAmountLimits {
            max_receive_msat: MAX_RECEIVE_AMOUNT_BETA_MSAT,
            liquidity_limit,
        }
    }
}
