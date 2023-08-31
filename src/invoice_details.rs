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
