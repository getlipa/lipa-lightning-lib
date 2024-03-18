use crate::{Amount, InvoiceDetails, OfferKind, PayErrorCode, SwapInfo, TzTime};
use std::time::SystemTime;

use breez_sdk_core::{LnPaymentDetails, PaymentStatus};

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

/// Information about a payment.
#[derive(PartialEq, Debug)]
pub struct Payment {
    pub payment_state: PaymentState,
    /// Hex representation of payment hash.
    pub hash: String,
    /// Actual amount payed or received, the value might change with payment status.
    pub amount: Amount,
    pub invoice_details: InvoiceDetails,
    pub created_at: TzTime,
    /// The description embedded in the invoice. Given the length limit of this data,
    /// it is possible that a hex hash of the description is provided instead, but that is uncommon.
    pub description: String,
    /// Hex representation of the preimage. Will only be present on successful payments.
    pub preimage: Option<String>,
    /// A personal note previously added to this payment through [`LightningNode::set_payment_personal_note`](crate::LightningNode::set_payment_personal_note)
    pub personal_note: Option<String>,
    pub details: PaymentDetails,
}

#[derive(PartialEq, Debug)]
pub enum PaymentDetails {
    Incoming(IncomingPayment),
    Outgoing(OutgoingPayment),
}

/// Information specific to an incoming payment.
#[derive(PartialEq, Debug)]
pub struct IncomingPayment {
    /// Nominal amount specified in the invoice.
    pub requested_amount: Amount,
    /// LSP fees paid in a payment. The amount is only paid if successful.
    pub lsp_fees: Amount,

	// Here we need recipient, if it is a lightning address or a phone number.
	// Or we can move `OutgoingPayment::recipient` to `Payment`.

	// Merge into one thing?
    /// The swap information of a payment if triggered by a swap.
    pub swap: Option<SwapInfo>,
	// We will have Referrals here also.
    /// An offer a payment came from if any.
    pub offer: Option<OfferKind>,
}

/// Information specific to an outgoing payment.
#[derive(PartialEq, Debug)]
pub struct OutgoingPayment {
    /// Routing fees paid in a payment. Will only be present if the payment was successful.
    pub network_fees: Amount,
	// Here we can encode debit card topups.
    /// Information about a payment's recipient.
    pub recipient: Recipient,
}

/// User-friendly representation of an outgoing payment's recipient.
#[derive(PartialEq, Debug)]
pub enum Recipient {
    LightningAddress { address: String },
    LnUrlPayDomain { domain: String },
    Unknown,
}

impl Recipient {
    pub(crate) fn new(payment_details: &LnPaymentDetails) -> Recipient {
        if let Some(address) = &payment_details.ln_address {
            Recipient::LightningAddress {
                address: address.to_string(),
            }
        } else if let Some(lnurlp_domain) = &payment_details.lnurl_pay_domain {
            Recipient::LnUrlPayDomain {
                domain: lnurlp_domain.to_string(),
            }
        } else {
            Recipient::Unknown
        }
    }
}

/// Information about **all** pending and **only** requested completed activities.
pub struct ListActivitiesResponse {
    pub pending_activities: Vec<Activity>,
    pub completed_activities: Vec<Activity>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum Activity {
    PaymentActivity { payment: Payment },
    ChannelCloseActivity { channel_close: ChannelClose },
}

impl Activity {
    pub(crate) fn get_time(&self) -> SystemTime {
        match self {
            Activity::PaymentActivity { payment } => payment.created_at.time,
            Activity::ChannelCloseActivity { channel_close } => channel_close
                .closed_at
                .clone()
                .map(|t| t.time)
                .unwrap_or(SystemTime::now()),
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        match self {
            Activity::PaymentActivity { payment } => matches!(
                payment.payment_state,
                PaymentState::Created | PaymentState::Retried
            ),
            Activity::ChannelCloseActivity { channel_close } => match channel_close.state {
                ChannelCloseState::Pending => true,
                ChannelCloseState::Confirmed => false,
            },
        }
    }
}

/// Information about a closed channel.
#[derive(Debug, PartialEq)]
pub struct ChannelClose {
    /// Our balance on the channel that got closed.
    pub amount: Amount,
    pub state: ChannelCloseState,
    /// When the channel closing tx got confirmed. For pending channel closes, this will be empty.
    pub closed_at: Option<TzTime>,
    pub closing_tx_id: String,
}

#[derive(Debug, PartialEq)]
pub enum ChannelCloseState {
    Pending,
    Confirmed,
}
