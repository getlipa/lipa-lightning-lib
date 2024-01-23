use crate::amount::{AsSats, ToAmount};
use crate::errors::DecodeDataError;
use crate::{Amount, ExchangeRate};
use breez_sdk_core::{LnUrlPayRequestData, LnUrlWithdrawRequestData, MetadataItem};
use perro::ensure;

/// Information about an LNURL-pay.
pub struct LnUrlPayDetails {
    /// The domain of the LNURL-pay service, to be shown to the user when asking for
    /// payment input, as per LUD-06 spec.
    pub domain: String,
    pub short_description: String,
    pub long_description: Option<String>,
    pub min_sendable: Amount,
    pub max_sendable: Amount,
    /// An internal struct is not supposed to be inspected, but only passed to [`crate::LightningNode::pay_lnurlp`].
    pub request_data: LnUrlPayRequestData,
}

impl LnUrlPayDetails {
    pub(crate) fn from_lnurl_pay_request_data(
        request_data: LnUrlPayRequestData,
        exchange_rate: &Option<ExchangeRate>,
    ) -> std::result::Result<Self, DecodeDataError> {
        let mut short_description = String::new();
        let mut long_description = None;
        let metadata = request_data
            .metadata_vec()
            .map_err(|e| DecodeDataError::LnUrlError { msg: e.to_string() })?;
        for MetadataItem { key, value } in metadata {
            match key.as_str() {
                "text/plain" => short_description = value.clone(),
                "text/long-desc" => long_description = Some(value.clone()),
                _ => (),
            }
        }
        ensure!(
            !short_description.is_empty(),
            DecodeDataError::LnUrlError {
                msg: "Missing short description".to_string()
            }
        );
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
                .max_withdrawable
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
