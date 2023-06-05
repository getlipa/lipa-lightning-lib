use crate::interfaces::ExchangeRate;
use lightning_invoice::Invoice;

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
pub struct FiatValues {
    pub fiat: String,
    pub amount: u64,
    pub amount_usd: u64,
}

impl FiatValues {
    pub fn from_amount_msat(
        amount_msat: u64,
        exchange_rate: &ExchangeRate,
        exchange_rate_usd: &ExchangeRate,
    ) -> Self {
        // Fiat amount in thousandths of the major fiat unit.
        let amount = amount_msat / (exchange_rate.rate as u64);
        let amount_usd = amount_msat / (exchange_rate_usd.rate as u64);
        FiatValues {
            fiat: exchange_rate.currency_code.clone(),
            amount,
            amount_usd,
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Payment {
    pub payment_type: PaymentType,
    pub payment_state: PaymentState,
    pub hash: String,
    pub amount_msat: u64,
    pub invoice: Invoice,
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
