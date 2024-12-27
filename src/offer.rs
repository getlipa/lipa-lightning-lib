use crate::amount::{Amount, AsSats, ToAmount};
use crate::exchange_rate_provider::ExchangeRate;
use crate::PocketOfferError;

use crow::{TopupInfo, TopupStatus};
use std::time::SystemTime;

#[derive(Debug, PartialEq, Clone, Eq)]
pub enum OfferStatus {
    READY,
    /// Claiming the offer failed, but it can be retried.
    FAILED,
    /// The offer could not be claimed, so the user got refunded.
    /// Specific info for Pocket offers:
    /// - The Refund happened over the Fiat rails
    /// - Reasons for why the offer was refunded: <https://pocketbitcoin.com/developers/docs/rest/v1/webhooks#refund-reasons>
    REFUNDED,
    SETTLED,
}

/// Values are denominated in the fiat currency the user sent to the exchange.
/// The currency code can be found in `exchange_rate`.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Offer {
    pub id: String,
    /// The exchange rate used by the exchange to exchange fiat to sats.
    pub exchange_rate: ExchangeRate,
    /// The original fiat amount sent to the exchange.
    pub topup_value_minor_units: u64,
    /// The sat amount after the exchange. Isn't available for topups collected before version v0.30.0-beta.
    pub topup_value_sats: Option<u64>,
    /// The fee paid to perform the exchange from fiat to sats.
    pub exchange_fee_minor_units: u64,
    /// The rate of the fee expressed in permyriad (e.g. 1.5% would be 150).
    pub exchange_fee_rate_permyriad: u16,
    /// Optional payout fees collected by pocket.
    pub lightning_payout_fee: Option<Amount>,
    /// The optional error that might have occurred in the offer withdrawal process.
    pub error: Option<PocketOfferError>,
}

/// Information on a funds offer that can be claimed
/// using [`crate::LightningNode::request_offer_collection`].
#[derive(Debug, PartialEq, Clone, Eq)]
pub struct OfferInfo {
    pub offer: Offer,
    /// Amount available for withdrawal
    pub amount: Amount,
    /// The lnurlw string that will be used to withdraw this offer. Can be empty if the offer isn't
    /// available anymore (i.e `status` is [`OfferStatus::REFUNDED`])
    pub lnurlw: Option<String>,
    pub created_at: SystemTime,
    /// The time this offer expires at. Can be empty if the offer isn't available anymore
    /// (i.e `status` is [`OfferStatus::REFUNDED`]).
    pub expires_at: Option<SystemTime>,
    pub status: OfferStatus,
}

impl OfferInfo {
    pub(crate) fn from(topup_info: TopupInfo, current_rate: &Option<ExchangeRate>) -> OfferInfo {
        let exchange_rate = ExchangeRate {
            currency_code: topup_info.exchange_rate.currency_code,
            rate: topup_info.exchange_rate.sats_per_unit,
            updated_at: topup_info.exchange_rate.updated_at,
        };

        let status = match topup_info.status {
            TopupStatus::READY => OfferStatus::READY,
            TopupStatus::FAILED => OfferStatus::FAILED,
            TopupStatus::REFUNDED => OfferStatus::REFUNDED,
            TopupStatus::SETTLED => OfferStatus::SETTLED,
        };

        OfferInfo {
            offer: Offer {
                id: topup_info.id,
                exchange_rate,
                topup_value_minor_units: topup_info.topup_value_minor_units,
                topup_value_sats: Some(topup_info.amount_sat),
                exchange_fee_minor_units: topup_info.exchange_fee_minor_units,
                exchange_fee_rate_permyriad: topup_info.exchange_fee_rate_permyriad,
                lightning_payout_fee: None,
                error: topup_info.error,
            },
            amount: topup_info.amount_sat.as_sats().to_amount_down(current_rate),
            lnurlw: topup_info.lnurlw,
            created_at: topup_info.exchange_rate.updated_at,
            expires_at: topup_info.expires_at,
            status,
        }
    }
}
