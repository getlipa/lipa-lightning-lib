use crate::amount::Amount;

use std::time::{Duration, SystemTime};

// TODO remove dead code after breez sdk implementation
#[allow(dead_code)]
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
