use crate::Amount;
use breez_sdk_core::ReverseSwapStatus;

/// Information about a successful reverse swap.
#[derive(PartialEq, Debug)]
pub struct ReverseSwapInfo {
    pub paid_onchain_amount: Amount,
    /// The tx id of the claim tx, which is the final tx in the reverse swap flow, which send funds
    /// to the targeted on-chain address.
    ///
    /// It will only be present once the claim tx is broacasted.
    pub claim_txid: Option<String>,
    pub status: ReverseSwapStatus,
}
