use crate::Amount;
use breez_sdk_core::ReverseSwapStatus;

/// Information about a successful reverse swap.
#[derive(PartialEq, Debug)]
pub struct ReverseSwapInfo {
    pub paid_onchain_amount: Amount,
    /// Total fees paid excluding LN routing fees. Includes onchain
    /// fees and reverse-swap provider fees.
    pub swap_fees_amount: Amount,
    /// The tx id of the claim tx, which is the final tx in the reverse swap flow, which send funds
    /// to the targeted on-chain address.
    ///
    /// It will only be present once the claim tx is broadcasted.
    pub claim_txid: Option<String>,
    pub status: ReverseSwapStatus,
}
