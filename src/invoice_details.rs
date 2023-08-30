use crate::amount::{Amount, ToAmount};

use crate::ExchangeRate;
use lightning::offers::invoice::Invoice;
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
    pub(crate) fn from_local_invoice(invoice: Invoice, rate: &Option<ExchangeRate>) -> Self {
        todo!()
    }

    pub(crate) fn from_remote_invoice(invoice: Invoice, rate: &Option<ExchangeRate>) -> Self {
        todo!()
    }
}

