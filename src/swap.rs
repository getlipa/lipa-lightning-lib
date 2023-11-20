use crate::amount::Amount;
use crate::config::TzTime;

use std::time::SystemTime;

/// Information about a successful swap.
#[derive(PartialEq, Debug)]
pub struct SwapInfo {
    pub bitcoin_address: String,
    pub created_at: TzTime,
    pub paid_sats: u64,
}

/// Information about a generated swap address
pub struct SwapAddressInfo {
    /// Funds sent to this address will be swapped into LN to be received by the local wallet
    pub address: String,
    /// Minimum amount to be sent to `address`
    pub min_deposit: Amount,
    /// Maximum amount to be sent to `address`
    pub max_deposit: Amount,
}

/// Information about a failed swap
pub struct FailedSwapInfo {
    pub address: String,
    /// The amount that is available to be recovered. The recovery will involve paying some
    /// on-chain fees so it isn't possible to recover the entire amount.
    pub amount: Amount,
    pub created_at: SystemTime,
}

/// Information the resolution of a failed swap.
pub struct ResolveFailedSwapInfo {
    /// The address of the failed swap.
    pub swap_address: String,
    /// The amount that will be sent (swap amount - onchain fee).
    pub recovered_amount: Amount,
    /// The amount that will be paid in onchain fees.
    pub onchain_fee: Amount,
    /// The address to which recovered funds will be sent.
    pub to_address: String,
    /// The onchain fee rate that will be applied. This fee rate results in the `onchain_fee`.
    pub onchain_fee_rate: u32,
}