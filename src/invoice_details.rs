use crate::amount::{Amount, AsSats, ToAmount};

use crate::util::unix_timestamp_to_system_time;
use crate::ExchangeRate;
use breez_sdk_core::LNInvoice;
use std::time::{Duration, SystemTime};

/// Information embedded in an invoice
pub struct InvoiceDetails {
    /// The BOLT11 invoice.
    pub invoice: String,
    /// Payment amount, if specified. If not available, invoice is an open-amount invoice
    /// and the user should be prompted for how much they want to pay.
    /// The fiat value is calculated base on the "natural" exchange rate:
    ///  - for a new invoice current exchange rate is used
    ///  - for old invoices historic values are used
    pub amount: Option<Amount>,
    pub description: String,
    pub payment_hash: String,
    /// The pubkey (aka node id) of the invoice issuer. Please keep in mind that this doesn't necessarily
    /// identify the payee due to the proliferation of custodial wallets (multiple users will share a node id).
    pub payee_pub_key: String,
    /// The moment an invoice was created (UTC)
    pub creation_timestamp: SystemTime,
    /// The interval after which the invoice expires (creation_timestamp + expiry_interval = expiry_timestamp)
    pub expiry_interval: Duration,
    /// The moment an invoice expires (UTC)
    pub expiry_timestamp: SystemTime,
}

impl InvoiceDetails {
    pub(crate) fn from_ln_invoice(
        ln_invoice: LNInvoice,
        exchange_rate: &Option<ExchangeRate>,
    ) -> Self {
        InvoiceDetails {
            invoice: ln_invoice.bolt11,
            amount: ln_invoice
                .amount_msat
                .map(|a| a.as_msats().to_amount_down(exchange_rate)),
            description: ln_invoice.description.unwrap_or_default(),
            payment_hash: ln_invoice.payment_hash,
            payee_pub_key: ln_invoice.payee_pubkey,
            creation_timestamp: unix_timestamp_to_system_time(ln_invoice.timestamp),
            expiry_interval: Duration::from_secs(ln_invoice.expiry),
            expiry_timestamp: unix_timestamp_to_system_time(
                ln_invoice.timestamp + ln_invoice.expiry,
            ),
        }
    }
}
