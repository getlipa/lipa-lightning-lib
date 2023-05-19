use crate::interfaces::ExchangeRate;
use crate::InvoiceDetails;

use num_enum::TryFromPrimitive;
use std::time::SystemTime;

const MAX_RECEIVE_AMOUNT_BETA_SAT: u64 = 1_000_000;

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
    pub invoice_details: InvoiceDetails,
    pub created_at: TzTime,
    pub latest_state_change_at: TzTime,
    pub description: String,
    pub preimage: Option<String>,
    pub network_fees_msat: Option<u64>,
    pub lsp_fees_msat: Option<u64>,
    pub fiat_values: Option<FiatValues>,
    pub metadata: String,
}

impl Payment {
    pub(crate) fn has_expired(&self) -> bool {
        if self.invoice_details.expiry_timestamp < SystemTime::now() {
            return match self.payment_type {
                PaymentType::Receiving => self.payment_state == PaymentState::Created,
                PaymentType::Sending => self.payment_state == PaymentState::Failed,
            };
        }
        false
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct PaymentAmountLimits {
    pub max_receive_sat: u64,
    pub channel_related_limit: Option<ChannelRelatedLimit>,
}

impl PaymentAmountLimits {
    pub fn fetch(inbound_capacity: u64, lsp_min_fee: u64) -> Self {
        let min_receive_amount = lsp_min_fee * 2;

        let channel_related_limit = if inbound_capacity < min_receive_amount {
            Some(ChannelRelatedLimit {
                limit_type: AmountLimitType::MinReceive,
                amount_sat: min_receive_amount,
            })
        } else if inbound_capacity < MAX_RECEIVE_AMOUNT_BETA_SAT {
            Some(ChannelRelatedLimit {
                limit_type: AmountLimitType::MaxFreeReceive,
                amount_sat: inbound_capacity,
            })
        } else {
            None
        };

        PaymentAmountLimits {
            max_receive_sat: MAX_RECEIVE_AMOUNT_BETA_SAT,
            channel_related_limit,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct ChannelRelatedLimit {
    pub limit_type: AmountLimitType,
    pub amount_sat: u64,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum AmountLimitType {
    MaxFreeReceive,
    MinReceive,
}
