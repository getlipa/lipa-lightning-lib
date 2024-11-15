use crate::amount::{AsSats, ToAmount};
use crate::errors::{
    map_lnurl_pay_error, map_lnurl_withdraw_error, LnUrlWithdrawErrorCode, LnUrlWithdrawResult,
};
use crate::lightning::Payments;
use crate::support::Support;
use crate::{Amount, DecodeDataError, ExchangeRate, LnUrlPayErrorCode, LnUrlPayResult};
use breez_sdk_core::{
    LnUrlPayRequest, LnUrlPayRequestData, LnUrlWithdrawRequest, LnUrlWithdrawRequestData,
    MetadataItem,
};
use log::warn;
use perro::{ensure, invalid_input, runtime_error};
use std::sync::Arc;

pub struct Lnurl {
    support: Arc<Support>,
}

impl Lnurl {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Pay an LNURL-pay the provided amount.
    ///
    /// Parameters:
    /// * `lnurl_pay_request_data` - LNURL-pay request data as obtained from
    ///     [`LightningNode::decode_data`](crate::LightningNode::decode_data)
    /// * `amount_sat` - amount to be paid
    /// * `comment` - optional comment to be sent to payee (`max_comment_length` in
    ///     [`LnUrlPayDetails`] must be respected)
    ///
    /// Returns the payment hash of the payment.
    ///
    /// Requires network: **yes**
    pub fn pay(
        &self,
        lnurl_pay_request_data: LnUrlPayRequestData,
        amount_sat: u64,
        comment: Option<String>,
    ) -> LnUrlPayResult<String> {
        let comment_allowed = lnurl_pay_request_data.comment_allowed;
        ensure!(
            !matches!(comment, Some(ref comment) if comment.len() > comment_allowed as usize),
            invalid_input(format!(
                "The provided comment is longer than the allowed {comment_allowed} characters"
            ))
        );

        let payment_hash = match self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.lnurl_pay(LnUrlPayRequest {
                data: lnurl_pay_request_data,
                amount_msat: amount_sat.as_sats().msats,
                use_trampoline: true,
                comment,
                payment_label: None,
                validate_success_action_url: Some(false),
            }))
            .map_err(map_lnurl_pay_error)?
        {
            breez_sdk_core::lnurl::pay::LnUrlPayResult::EndpointSuccess { data } => {
                Ok(data.payment.id)
            }
            breez_sdk_core::lnurl::pay::LnUrlPayResult::EndpointError { data } => runtime_error!(
                LnUrlPayErrorCode::LnUrlServerError,
                "LNURL server returned error: {}",
                data.reason
            ),
            breez_sdk_core::lnurl::pay::LnUrlPayResult::PayError { data } => {
                self.report_send_payment_issue(data.payment_hash);
                runtime_error!(
                    LnUrlPayErrorCode::PaymentFailed,
                    "Paying invoice for LNURL pay failed: {}",
                    data.reason
                )
            }
        }?;
        self.store_payment_info(&payment_hash, None);
        Ok(payment_hash)
    }

    /// Withdraw an LNURL-withdraw the provided amount.
    ///
    /// A successful return means the LNURL-withdraw service has started a payment.
    /// Only after the event [`EventsCallback::payment_received`](crate::EventsCallback::payment_received) can the payment be considered
    /// received.
    ///
    /// Parameters:
    /// * `lnurl_withdraw_request_data` - LNURL-withdraw request data as obtained from [`LightningNode::decode_data`](crate::LightningNode::decode_data)
    /// * `amount_sat` - amount to be withdraw
    ///
    /// Returns the payment hash of the payment.
    ///
    /// Requires network: **yes**
    pub fn withdraw(
        &self,
        lnurl_withdraw_request_data: LnUrlWithdrawRequestData,
        amount_sat: u64,
    ) -> LnUrlWithdrawResult<String> {
        let payment_hash = match self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.lnurl_withdraw(LnUrlWithdrawRequest {
                data: lnurl_withdraw_request_data,
                amount_msat: amount_sat.as_sats().msats,
                description: None,
            }))
            .map_err(map_lnurl_withdraw_error)?
        {
            breez_sdk_core::LnUrlWithdrawResult::Ok { data } => Ok(data.invoice.payment_hash),
            breez_sdk_core::LnUrlWithdrawResult::Timeout { data } => {
                warn!("Tolerating timeout on submitting invoice to LNURL-w");
                Ok(data.invoice.payment_hash)
            }
            breez_sdk_core::LnUrlWithdrawResult::ErrorStatus { data } => runtime_error!(
                LnUrlWithdrawErrorCode::LnUrlServerError,
                "LNURL server returned error: {}",
                data.reason
            ),
        }?;
        self.store_payment_info(&payment_hash, None);
        Ok(payment_hash)
    }
}

impl Payments for Lnurl {
    fn support(&self) -> &Support {
        &self.support
    }
}

/// Information about an LNURL-pay.
pub struct LnUrlPayDetails {
    /// The domain of the LNURL-pay service, to be shown to the user when asking for
    /// payment input, as per LUD-06 spec.
    pub domain: String,
    pub short_description: String,
    pub long_description: Option<String>,
    pub min_sendable: Amount,
    pub max_sendable: Amount,
    pub max_comment_length: u16,
    /// An internal struct is not supposed to be inspected, but only passed to [`crate::LightningNode::pay_lnurlp`].
    pub request_data: LnUrlPayRequestData,
}

impl LnUrlPayDetails {
    pub(crate) fn from_lnurl_pay_request_data(
        request_data: LnUrlPayRequestData,
        exchange_rate: &Option<ExchangeRate>,
    ) -> Result<Self, DecodeDataError> {
        let (short_description, long_description) = parse_metadata(&request_data.metadata_str)
            .map_err(|msg| DecodeDataError::LnUrlError { msg })?;
        Ok(Self {
            domain: request_data.domain.clone(),
            short_description,
            long_description,
            min_sendable: request_data
                .min_sendable
                .as_msats()
                .to_amount_up(exchange_rate),
            max_sendable: request_data
                .max_sendable
                .as_msats()
                .to_amount_up(exchange_rate),
            max_comment_length: request_data.comment_allowed,
            request_data,
        })
    }
}

/// Information about an LNURL-withdraw.
pub struct LnUrlWithdrawDetails {
    pub min_withdrawable: Amount,
    pub max_withdrawable: Amount,
    /// An internal struct is not supposed to be inspected, but only passed to [`crate::LightningNode::withdraw_lnurlw`].
    pub request_data: LnUrlWithdrawRequestData,
}

impl LnUrlWithdrawDetails {
    pub(crate) fn from_lnurl_withdraw_request_data(
        request_data: LnUrlWithdrawRequestData,
        exchange_rate: &Option<ExchangeRate>,
    ) -> Self {
        Self {
            min_withdrawable: request_data
                .min_withdrawable
                .as_msats()
                .to_amount_up(exchange_rate),
            max_withdrawable: request_data
                .max_withdrawable
                .as_msats()
                .to_amount_up(exchange_rate),
            request_data,
        }
    }
}

pub(crate) fn parse_metadata(metadata: &str) -> Result<(String, Option<String>), String> {
    let metadata = serde_json::from_str::<Vec<MetadataItem>>(metadata)
        .map_err(|e| format!("Invalid metadata JSON: {e}"))?;
    let mut short_description = String::new();
    let mut long_description = None;
    for MetadataItem { key, value } in metadata {
        match key.as_str() {
            "text/plain" => short_description = value,
            "text/long-desc" => long_description = Some(value),
            _ => (),
        }
    }
    ensure!(
        !short_description.is_empty(),
        "Metadata missing short description".to_string()
    );
    Ok((short_description, long_description))
}
