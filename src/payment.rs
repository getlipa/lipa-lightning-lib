use crate::{Amount, InvoiceDetails, OfferKind, PayErrorCode, SwapInfo, TzTime};

use breez_sdk_core::PaymentStatus;

#[derive(PartialEq, Eq, Debug, Clone)]
#[repr(u8)]
pub enum PaymentType {
    Receiving,
    Sending,
}

#[derive(PartialEq, Eq, Debug, Clone)]
#[repr(u8)]
pub enum PaymentState {
    /// The payment was created and is in progress.
    Created,
    /// The payment succeeded.
    Succeeded,
    /// The payment failed. If it is a [`PaymentType::Sending`] payment, it can be retried.
    Failed,
    /// A payment retrial is in progress.
    Retried,
    /// The invoice associated with this payment has expired.
    InvoiceExpired,
}

impl From<PaymentStatus> for PaymentState {
    fn from(status: PaymentStatus) -> Self {
        match status {
            PaymentStatus::Pending => PaymentState::Created,
            PaymentStatus::Complete => PaymentState::Succeeded,
            PaymentStatus::Failed => PaymentState::Failed,
        }
    }
}

/// Information about an incoming or outgoing payment.
#[derive(PartialEq, Debug)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    /// For now, will always be empty.
    pub fail_reason: Option<PayErrorCode>,
    /// Hex representation of payment hash.
    pub hash: String,
    /// Actual amount payed or received, will equal the `requested_amount` until payment succeeded.
    pub amount: Amount,
    /// Nominal amount specified in the invoice.
    pub requested_amount: Amount,
    pub invoice_details: InvoiceDetails,
    pub created_at: TzTime,
    /// The description embedded in the invoice. Given the length limit of this data,
    /// it is possible that a hex hash of the description is provided instead, but that is uncommon.
    pub description: String,
    /// Hex representation of the preimage. Will only be present on successful payments.
    pub preimage: Option<String>,
    /// Routing fees paid in a [`PaymentType::Sending`] payment. Will only be present if the payment
    /// was successful.
    pub network_fees: Option<Amount>,
    /// LSP fees paid in a [`PaymentType::Receiving`] payment. Will never be present for
    /// [`PaymentType::Sending`] payments but might be 0 for [`PaymentType::Receiving`] payments.
    /// The amount is only paid if successful.
    pub lsp_fees: Option<Amount>,
    /// An offer a [`PaymentType::Receiving`] payment came from if any.
    pub offer: Option<OfferKind>,
    /// The swap information of a [`PaymentType::Receiving`] payment if triggered by a swap.
    pub swap: Option<SwapInfo>,
    /// A lightning address the payment has been sent to.
    pub lightning_address: Option<String>,
}
