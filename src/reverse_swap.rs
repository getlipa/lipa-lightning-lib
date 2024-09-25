use crate::Amount;
use breez_sdk_core::ReverseSwapStatus;

/// Information about a successful reverse swap.
#[derive(PartialEq, Debug)]
pub struct ReverseSwapInfo {
    pub paid_onchain_amount: Amount,
    /// Total fees paid excluding LN routing fees. In practice this doesn't include only onchain
    /// fees (the reverse-swap provider also takes a cut) but from the perspective of a payer,
    /// these are fees involved with paying onchain.
    pub onchain_fees_amount: Amount,
    /// The tx id of the claim tx, which is the final tx in the reverse swap flow, which send funds
    /// to the targeted on-chain address.
    ///
    /// It will only be present once the claim tx is broadcasted.
    pub claim_txid: Option<String>,
    pub status: ReverseSwapStatus,
}
