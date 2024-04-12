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
    pub max_comment_length: u16,
    /// An internal struct is not supposed to be inspected, but only passed to [`crate::LightningNode::pay_lnurlp`].
    pub request_data: LnUrlPayRequestData,
}

impl LnUrlPayDetails {
    pub(crate) fn from_lnurl_pay_request_data(
        request_data: LnUrlPayRequestData,
        exchange_rate: &Option<ExchangeRate>,
    ) -> std::result::Result<Self, DecodeDataError> {
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

pub(crate) fn parse_metadata(
    metadata: &str,
) -> std::result::Result<(String, Option<String>), String> {
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
