use crate::amount::{AsSats, ToAmount};
use crate::{Amount, ExchangeRate};
use breez_sdk_core::LnUrlPayRequestData;

/// Information about an LNURL-pay.
pub struct LnUrlPayDetails {
    pub min_sendable: Amount,
    pub max_sendable: Amount,
    /// An internal struct is not supposed to be inspected, but only passed to [`crate::LightningNode::pay_lnurlp`].
    pub request_data: LnUrlPayRequestData,
}

impl LnUrlPayDetails {
    pub(crate) fn from_lnurl_pay_request_data(
        request_data: LnUrlPayRequestData,
        exchange_rate: &Option<ExchangeRate>,
    ) -> Self {
        Self {
            min_sendable: request_data
                .min_sendable
                .as_msats()
                .to_amount_up(exchange_rate),
            max_sendable: request_data
                .max_sendable
                .as_msats()
                .to_amount_up(exchange_rate),
            request_data,
        }
    }
}
