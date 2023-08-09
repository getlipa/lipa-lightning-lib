use crate::interfaces::ExchangeRate;
use lightning_invoice::Bolt11Invoice;

use crate::errors::PayErrorCode;
use num_enum::TryFromPrimitive;
use std::time::SystemTime;

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PaymentType {
    Receiving,
    Sending,
}

#[derive(PartialEq, Eq, Debug, TryFromPrimitive, Clone)]
#[repr(u8)]
pub enum PaymentState {
    Created,
    Succeeded,
    Failed,
    Retried,
    InvoiceExpired,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct TzTime {
    pub time: SystemTime,
    pub timezone_id: String,
    pub timezone_utc_offset_secs: i32,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    pub fail_reason: Option<PayErrorCode>,
    pub hash: String,
    pub amount_msat: u64,
    pub invoice: Bolt11Invoice,
    pub created_at: TzTime,
    pub latest_state_change_at: TzTime,
    pub description: String,
    pub preimage: Option<String>,
    pub network_fees_msat: Option<u64>,
    pub lsp_fees_msat: Option<u64>,
    pub exchange_rate: Option<ExchangeRate>,
    pub metadata: String,
}

impl Payment {
    pub(crate) fn has_expired(&self) -> bool {
        if self.invoice.is_expired() {
            return match self.payment_type {
                PaymentType::Receiving => self.payment_state == PaymentState::Created,
                PaymentType::Sending => self.payment_state == PaymentState::Failed,
            };
        }
        false
    }
}
