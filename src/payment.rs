use std::ops::Add;

use crate::amount::{AsSats, ToAmount};
use crate::config::WithTimezone;
use crate::lightning::lnurl::parse_metadata;
use crate::phone_number::lightning_address_to_phone_number;
use crate::util::unix_timestamp_to_system_time;
use crate::{Amount, ExchangeRate, InvoiceDetails, Result, TzConfig, TzTime};

use breez_sdk_core::{parse_invoice, LnPaymentDetails, PaymentDetails, PaymentStatus};
use perro::{permanent_failure, MapToError};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
#[repr(u8)]
pub enum PaymentState {
    /// The payment was created and is in progress.
    Created,
    /// The payment succeeded.
    Succeeded,
    /// The payment failed. If it is an [`OutgoingPaymentInfo`], it can be retried.
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

impl PaymentState {
    pub(crate) fn is_pending(&self) -> bool {
        match self {
            PaymentState::Created | PaymentState::Retried => true,
            PaymentState::Succeeded | PaymentState::Failed | PaymentState::InvoiceExpired => false,
        }
    }
}

/// Information about a payment.
#[derive(PartialEq, Debug, Clone)]
pub struct PaymentInfo {
    pub payment_state: PaymentState,
    /// Hex representation of payment hash.
    pub hash: String,
    /// Actual amount payed or received.
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
}

impl PaymentInfo {
    pub(crate) fn new(
        breez_payment: breez_sdk_core::Payment,
        exchange_rate: &Option<ExchangeRate>,
        tz_config: TzConfig,
        personal_note: Option<String>,
    ) -> Result<Self> {
        let payment_details = match breez_payment.details {
            PaymentDetails::Ln { data } => data,
            PaymentDetails::ClosedChannel { .. } => {
                permanent_failure!("PaymentInfo cannot be created from channel close")
            }
        };
        let invoice = parse_invoice(&payment_details.bolt11).map_to_permanent_failure(format!(
            "Invalid invoice provided by the Breez SDK: {}",
            payment_details.bolt11
        ))?;
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice, exchange_rate);

        // Use invoice timestamp for receiving payments and breez_payment.payment_time for sending ones
        // Reasoning: for receiving payments, Breez returns the time the invoice was paid. Given that
        // now we show pending invoices, this can result in a receiving payment jumping around in the
        // list when it gets paid.
        let time = match breez_payment.payment_type {
            breez_sdk_core::PaymentType::Sent => {
                unix_timestamp_to_system_time(breez_payment.payment_time as u64)
            }
            breez_sdk_core::PaymentType::Received => invoice_details.creation_timestamp,
            breez_sdk_core::PaymentType::ClosedChannel => {
                permanent_failure!(
                    "Current interface doesn't support PaymentDetails::ClosedChannel"
                )
            }
        };
        let time = time.with_timezone(tz_config.clone());

        let amount = match breez_payment.payment_type {
            breez_sdk_core::PaymentType::Sent => (breez_payment.amount_msat
                + breez_payment.fee_msat)
                .as_msats()
                .to_amount_up(exchange_rate),
            breez_sdk_core::PaymentType::Received => breez_payment
                .amount_msat
                .as_msats()
                .to_amount_down(exchange_rate),
            breez_sdk_core::PaymentType::ClosedChannel => {
                permanent_failure!("PaymentInfo cannot be created from channel close")
            }
        };

        let description = match payment_details
            .lnurl_metadata
            .as_deref()
            .map(parse_metadata)
        {
            Some(Ok((short_description, _long_description))) => short_description,
            _ => invoice_details.description.clone(),
        };

        let preimage = if payment_details.payment_preimage.is_empty() {
            None
        } else {
            Some(payment_details.payment_preimage)
        };

        Ok(PaymentInfo {
            payment_state: breez_payment.status.into(),
            hash: payment_details.payment_hash,
            amount,
            invoice_details,
            created_at: time,
            description,
            preimage,
            personal_note,
        })
    }
}

/// Information about an incoming payment.
#[derive(PartialEq, Debug)]
pub struct IncomingPaymentInfo {
    pub payment_info: PaymentInfo,
    /// Nominal amount specified in the invoice.
    pub requested_amount: Amount,
    /// LSP fees paid. The amount is only paid if successful.
    pub lsp_fees: Amount,
    /// Which Lightning Address / Phone number this payment was received on.
    pub received_on: Option<Recipient>,
    /// Optional comment sent by the payer of an LNURL payment.
    pub received_lnurl_comment: Option<String>,
}

impl IncomingPaymentInfo {
    pub(crate) fn new(
        breez_payment: breez_sdk_core::Payment,
        exchange_rate: &Option<ExchangeRate>,
        tz_config: TzConfig,
        personal_note: Option<String>,
        received_on: Option<String>,
        received_lnurl_comment: Option<String>,
        lipa_lightning_domain: &str,
    ) -> Result<Self> {
        let lsp_fees = breez_payment
            .fee_msat
            .as_msats()
            .to_amount_up(exchange_rate);
        let requested_amount = breez_payment
            .amount_msat
            .add(breez_payment.fee_msat)
            .as_msats()
            .to_amount_down(exchange_rate);
        let payment_info =
            PaymentInfo::new(breez_payment, exchange_rate, tz_config, personal_note)?;
        let received_on =
            received_on.map(|r| Recipient::from_lightning_address(&r, lipa_lightning_domain));
        Ok(Self {
            payment_info,
            requested_amount,
            lsp_fees,
            received_on,
            received_lnurl_comment,
        })
    }
}

/// Information about an outgoing payment.
#[derive(PartialEq, Debug)]
pub struct OutgoingPaymentInfo {
    pub payment_info: PaymentInfo,
    /// Routing fees paid. Will only be present if the payment was successful.
    pub network_fees: Amount,
    /// Information about a payment's recipient.
    pub recipient: Recipient,
    /// Comment sent to the recipient.
    /// Only set for LNURL-pay and lightning address payments where a comment has been sent.
    pub comment_for_recipient: Option<String>,
}

impl OutgoingPaymentInfo {
    pub(crate) fn new(
        breez_payment: breez_sdk_core::Payment,
        exchange_rate: &Option<ExchangeRate>,
        tz_config: TzConfig,
        personal_note: Option<String>,
        lipa_lightning_domain: &str,
    ) -> Result<Self> {
        let network_fees = breez_payment
            .fee_msat
            .as_msats()
            .to_amount_up(exchange_rate);
        let data = match breez_payment.details {
            PaymentDetails::Ln { ref data } => data,
            PaymentDetails::ClosedChannel { .. } => {
                permanent_failure!("OutgoingPaymentInfo cannot be created from channel close")
            }
        };
        let recipient = Recipient::from_ln_payment_details(data, lipa_lightning_domain);
        let comment_for_recipient = data.lnurl_pay_comment.clone();
        let payment_info =
            PaymentInfo::new(breez_payment, exchange_rate, tz_config, personal_note)?;
        Ok(Self {
            payment_info,
            network_fees,
            recipient,
            comment_for_recipient,
        })
    }
}

/// User-friendly representation of an outgoing payment's recipient.
#[derive(PartialEq, Debug)]
pub enum Recipient {
    LightningAddress { address: String },
    LnUrlPayDomain { domain: String },
    PhoneNumber { e164: String },
    Unknown,
}

impl Recipient {
    pub(crate) fn from_ln_payment_details(
        payment_details: &LnPaymentDetails,
        lipa_lightning_domain: &str,
    ) -> Self {
        if let Some(address) = &payment_details.ln_address {
            if let Some(e164) = lightning_address_to_phone_number(address, lipa_lightning_domain) {
                return Recipient::PhoneNumber { e164 };
            }
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

    pub(crate) fn from_lightning_address(address: &str, lipa_lightning_domain: &str) -> Self {
        match lightning_address_to_phone_number(address, lipa_lightning_domain) {
            Some(e164) => Recipient::PhoneNumber { e164 },
            None => Recipient::LightningAddress {
                address: address.to_string(),
            },
        }
    }
}
