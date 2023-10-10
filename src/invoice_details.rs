use crate::amount::{Amount, ToAmount};

use crate::util::unix_timestamp_to_system_time;
use crate::ExchangeRate;
use breez_sdk_core::LNInvoice;
use std::time::{Duration, SystemTime};

pub struct InvoiceDetails {
    pub invoice: String,
    pub amount: Option<Amount>,
    pub description: String,
    pub payment_hash: String,
    pub payee_pub_key: String,
    pub creation_timestamp: SystemTime,
    pub expiry_interval: Duration,
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
                .map(|a| a.to_amount_down(exchange_rate)),
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
