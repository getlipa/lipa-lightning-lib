use crate::amount::{AsSats, ToAmount};
use crate::errors::Result;
use crate::locker::Locker;
use crate::pocketclient::FiatTopupInfo;
use crate::support::Support;
use crate::{
    filter_out_and_log_corrupted_activities, Activities, Activity, Amount, OfferInfo, OfferKind,
    OfferStatus, PaymentState, RuntimeErrorCode,
};
use breez_sdk_core::{
    parse, InputType, ListPaymentsRequest, LnUrlWithdrawRequest, PaymentStatus, PaymentTypeFilter,
    SignMessageRequest,
};
use crow::TopupInfo;
use email_address::EmailAddress;
use honeybadger::{TermsAndConditions, TermsAndConditionsStatus};
use iban::Iban;
use log::debug;
use perro::{
    ensure, invalid_input, permanent_failure, runtime_error, MapToError, OptionToError, ResultTrait,
};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

pub struct FiatTopup {
    support: Arc<Support>,
    activities: Arc<Activities>,
}

impl FiatTopup {
    pub(crate) fn new(support: Arc<Support>, activities: Arc<Activities>) -> Self {
        Self {
            support,
            activities,
        }
    }

    /// Accepts Pocket's T&C.
    ///
    /// Parameters:
    /// * `version` - the version number being accepted.
    /// * `fingerprint` - the fingerprint of the version being accepted.
    ///
    /// Requires network: **yes**
    pub fn accept_tc(&self, version: i64, fingerprint: String) -> Result<()> {
        self.support
            .auth
            .accept_terms_and_conditions(TermsAndConditions::Pocket, version, fingerprint)
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
    }

    /// Query for the current T&C status.
    ///
    /// Requires network: **yes**
    pub fn query_tc_status(&self) -> Result<TermsAndConditionsStatus> {
        self.support
            .auth
            .get_terms_and_conditions_status(TermsAndConditions::Pocket)
            .map_runtime_error_to(RuntimeErrorCode::AuthServiceUnavailable)
    }

    /// Register for fiat topups. Returns information that can be used by the user to transfer fiat
    /// to the 3rd party exchange service. Once the 3rd party exchange receives funds, the user will
    /// be able to withdraw sats using LNURL-w.
    ///
    /// Parameters:
    /// * `email` - this email will be used to send status information about different topups
    /// * `user_iban` - the user will send fiat from this iban
    /// * `user_currency` - the fiat currency (ISO 4217 currency code) that will be sent for
    ///    exchange. Not all are supported. A consumer of this library should find out about available
    ///    ones using other sources.
    ///
    /// Requires network: **yes**
    pub fn register(
        &self,
        email: Option<String>,
        referral: Option<String>,
        user_iban: String,
        user_currency: String,
    ) -> Result<FiatTopupInfo> {
        debug!("fiat_topup().register() - called with - email: {email:?} - referral code: {referral:?} - user_iban: {user_iban} - user_currency: {user_currency:?}");
        user_iban
            .parse::<Iban>()
            .map_to_invalid_input("Invalid user_iban")?;

        if let Some(email) = email.as_ref() {
            EmailAddress::from_str(email).map_to_invalid_input("Invalid email")?;
        }

        if let Some(referral) = referral.as_ref() {
            let string_length = referral.len();
            if referral.len() > self.support.node_config.topup_referral_code_max_length as usize {
                invalid_input!("Invalid referral code [string length: {string_length}]");
            }
        }

        let sdk = Arc::clone(&self.support.sdk);
        let sign_message = |message| async move {
            sdk.sign_message(SignMessageRequest { message })
                .await
                .ok()
                .map(|r| r.signature)
        };
        let topup_info = self
            .support
            .rt
            .handle()
            .block_on(self.support.fiat_topup_client.register_pocket_fiat_topup(
                &user_iban,
                user_currency,
                self.support.get_node_info()?.node_pubkey,
                sign_message,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to register pocket fiat topup",
            )?;

        self.support
            .data_store
            .lock_unwrap()
            .store_fiat_topup_info(topup_info.clone())?;

        self.support
            .offer_manager
            .register_topup(topup_info.order_id.clone(), email, referral)
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;

        Ok(topup_info)
    }

    /// Resets a previous fiat topup registration.
    ///
    /// Requires network: **no**
    pub fn reset(&self) -> Result<()> {
        self.support
            .data_store
            .lock_unwrap()
            .clear_fiat_topup_info()
    }

    /// Returns the latest [`FiatTopupInfo`] if the user has registered for the fiat topup.
    ///
    /// Requires network: **no**
    pub fn get_info(&self) -> Result<Option<FiatTopupInfo>> {
        self.support
            .data_store
            .lock_unwrap()
            .retrieve_latest_fiat_topup_info()
    }

    /// Query all unclaimed fund offers
    ///
    /// Requires network: **yes**
    pub(crate) fn query_uncompleted_offers(&self) -> Result<Vec<OfferInfo>> {
        let topup_infos = self
            .support
            .offer_manager
            .query_uncompleted_topups()
            .map_runtime_error_to(RuntimeErrorCode::OfferServiceUnavailable)?;
        let rate = self.support.get_exchange_rate();

        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![PaymentTypeFilter::Received]),
            metadata_filters: None,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(false),
            limit: Some(5),
            offset: None,
        };
        let latest_activities = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .filter(|p| p.status == PaymentStatus::Complete)
            .map(|p| self.activities.activity_from_breez_payment(p))
            .filter_map(filter_out_and_log_corrupted_activities)
            .collect::<Vec<_>>();

        Ok(
            filter_out_recently_claimed_topups(topup_infos, latest_activities)
                .into_iter()
                .map(|topup_info| OfferInfo::from(topup_info, &rate))
                .collect(),
        )
    }

    /// Calculates the payout fee for an uncompleted offer.
    ///
    /// Parameters:
    /// * `offer` - An uncompleted offer for which the lightning payout fee should get calculated.
    ///
    /// Requires network: **yes**
    pub fn calculate_payout_fee(&self, offer: OfferInfo) -> Result<Amount> {
        ensure!(
            offer.status != OfferStatus::REFUNDED && offer.status != OfferStatus::SETTLED,
            invalid_input(format!("Provided offer is already completed: {offer:?}"))
        );

        let max_withdrawable_msats = match self.support.rt.handle().block_on(parse(
            &offer
                .lnurlw
                .ok_or_permanent_failure("Uncompleted offer didn't include an lnurlw")?,
        )) {
            Ok(InputType::LnUrlWithdraw { data }) => data,
            Ok(input_type) => {
                permanent_failure!("Invalid input type LNURLw in uncompleted offer: {input_type:?}")
            }
            Err(err) => {
                permanent_failure!("Invalid LNURLw in uncompleted offer: {err}")
            }
        }
        .max_withdrawable;

        ensure!(
            max_withdrawable_msats <= offer.amount.sats.as_sats().msats,
            permanent_failure("LNURLw provides more")
        );

        let exchange_rate = self.support.get_exchange_rate();

        Ok((offer.amount.sats.as_sats().msats - max_withdrawable_msats)
            .as_msats()
            .to_amount_up(&exchange_rate))
    }

    /// Request to collect the offer (e.g. a Pocket topup).
    /// A payment hash will be returned to track incoming payment.
    /// The offer collection might be considered successful once
    /// [`EventsCallback::payment_received`](crate::EventsCallback::payment_received) is called,
    /// or the [`PaymentState`] of the respective payment becomes [`PaymentState::Succeeded`].
    ///
    /// Parameters:
    /// * `offer` - An offer that is still valid for collection. Must have its `lnurlw` field
    ///   filled in.
    ///
    /// Requires network: **yes**
    pub fn request_collection(&self, offer: OfferInfo) -> Result<String> {
        let lnurlw_data = match self.support.rt.handle().block_on(parse(
            &offer
                .lnurlw
                .ok_or_invalid_input("The provided offer didn't include an lnurlw")?,
        )) {
            Ok(InputType::LnUrlWithdraw { data }) => data,
            Ok(input_type) => {
                permanent_failure!("Invalid input type LNURLw in offer: {input_type:?}")
            }
            Err(err) => permanent_failure!("Invalid LNURLw in offer: {err}"),
        };
        let collectable_amount = lnurlw_data.max_withdrawable;
        let hash = match self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.lnurl_withdraw(LnUrlWithdrawRequest {
                data: lnurlw_data,
                amount_msat: collectable_amount,
                description: None,
            })) {
            Ok(breez_sdk_core::LnUrlWithdrawResult::Ok { data }) => data.invoice.payment_hash,
            Ok(breez_sdk_core::LnUrlWithdrawResult::Timeout { .. }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to timeout on submitting invoice"
            ),
            Ok(breez_sdk_core::LnUrlWithdrawResult::ErrorStatus { data }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {}",
                data.reason
            ),
            Err(breez_sdk_core::LnUrlWithdrawError::Generic { err }) => runtime_error!(
                RuntimeErrorCode::OfferServiceUnavailable,
                "Failed to withdraw offer due to: {err}"
            ),
            Err(breez_sdk_core::LnUrlWithdrawError::InvalidAmount { err }) => {
                permanent_failure!("Invalid amount in invoice for LNURL withdraw: {err}")
            }
            Err(breez_sdk_core::LnUrlWithdrawError::InvalidInvoice { err }) => {
                permanent_failure!("Invalid invoice for LNURL withdraw: {err}")
            }
            Err(breez_sdk_core::LnUrlWithdrawError::InvalidUri { err }) => {
                permanent_failure!("Invalid URL in LNURL withdraw: {err}")
            }
            Err(breez_sdk_core::LnUrlWithdrawError::ServiceConnectivity { err }) => {
                runtime_error!(
                    RuntimeErrorCode::OfferServiceUnavailable,
                    "Failed to withdraw offer due to: {err}"
                )
            }
            Err(breez_sdk_core::LnUrlWithdrawError::InvoiceNoRoutingHints { err }) => {
                permanent_failure!(
                    "A locally created invoice doesn't have any routing hints: {err}"
                )
            }
        };

        // MOCK: We need to simulate the backend receiving an update from Pocket that the offer has been settled.
        #[allow(irrefutable_let_patterns)]
        #[cfg(feature = "mock-deps")]
        if let OfferKind::Pocket { id, .. } = offer.offer_kind.clone() {
            self.support.offer_manager.hide_topup(id).unwrap();
        }

        self.support
            .store_payment_info(&hash, Some(offer.offer_kind));

        Ok(hash)
    }
}

fn filter_out_recently_claimed_topups(
    topups: Vec<TopupInfo>,
    latest_activities: Vec<Activity>,
) -> Vec<TopupInfo> {
    let pocket_id = |a: Activity| match a {
        Activity::OfferClaim {
            incoming_payment_info: _,
            offer_kind: OfferKind::Pocket { id, .. },
        } => Some(id),
        _ => None,
    };
    let latest_succeeded_payment_offer_ids: HashSet<String> = latest_activities
        .into_iter()
        .filter(|a| a.get_payment_info().map(|p| p.payment_state) == Some(PaymentState::Succeeded))
        .filter_map(pocket_id)
        .collect();
    topups
        .into_iter()
        .filter(|o| !latest_succeeded_payment_offer_ids.contains(&o.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::fiat_topup::filter_out_recently_claimed_topups;
    use crate::node_config::WithTimezone;
    use crate::{
        Activity, Amount, ExchangeRate, IncomingPaymentInfo, InvoiceDetails, OfferKind,
        PaymentInfo, PaymentState, TzConfig,
    };
    use crow::{TopupInfo, TopupStatus};
    use std::time::SystemTime;

    #[test]
    fn test_filter_out_recently_claimed_topups() {
        let topups = vec![
            TopupInfo {
                id: "123".to_string(),
                status: TopupStatus::READY,
                amount_sat: 0,
                topup_value_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                exchange_fee_minor_units: 0,
                exchange_rate: graphql::ExchangeRate {
                    currency_code: "eur".to_string(),
                    sats_per_unit: 0,
                    updated_at: SystemTime::now(),
                },
                expires_at: None,
                lnurlw: None,
                error: None,
            },
            TopupInfo {
                id: "234".to_string(),
                status: TopupStatus::READY,
                amount_sat: 0,
                topup_value_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                exchange_fee_minor_units: 0,
                exchange_rate: graphql::ExchangeRate {
                    currency_code: "eur".to_string(),
                    sats_per_unit: 0,
                    updated_at: SystemTime::now(),
                },
                expires_at: None,
                lnurlw: None,
                error: None,
            },
        ];

        let mut payment_info = PaymentInfo {
            payment_state: PaymentState::Succeeded,
            hash: "hash".to_string(),
            amount: Amount::default(),
            invoice_details: InvoiceDetails {
                invoice: "bca".to_string(),
                amount: None,
                description: "".to_string(),
                payment_hash: "".to_string(),
                payee_pub_key: "".to_string(),
                creation_timestamp: SystemTime::now(),
                expiry_interval: Default::default(),
                expiry_timestamp: SystemTime::now(),
            },
            created_at: SystemTime::now().with_timezone(TzConfig::default()),
            description: "".to_string(),
            preimage: None,
            personal_note: None,
        };

        let incoming_payment = Activity::IncomingPayment {
            incoming_payment_info: IncomingPaymentInfo {
                payment_info: payment_info.clone(),
                requested_amount: Amount::default(),
                lsp_fees: Amount::default(),
                received_on: None,
                received_lnurl_comment: None,
            },
        };

        payment_info.hash = "hash2".to_string();
        let topup = Activity::OfferClaim {
            incoming_payment_info: IncomingPaymentInfo {
                payment_info: payment_info.clone(),
                requested_amount: Amount::default(),
                lsp_fees: Amount::default(),
                received_on: None,
                received_lnurl_comment: None,
            },
            offer_kind: OfferKind::Pocket {
                id: "123".to_string(),
                exchange_rate: ExchangeRate {
                    currency_code: "".to_string(),
                    rate: 0,
                    updated_at: SystemTime::now(),
                },
                topup_value_minor_units: 0,
                topup_value_sats: Some(0),
                exchange_fee_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                lightning_payout_fee: None,
                error: None,
            },
        };

        payment_info.hash = "hash3".to_string();
        payment_info.payment_state = PaymentState::Failed;
        let failed_topup = Activity::OfferClaim {
            incoming_payment_info: IncomingPaymentInfo {
                payment_info,
                requested_amount: Amount::default(),
                lsp_fees: Amount::default(),
                received_on: None,
                received_lnurl_comment: None,
            },
            offer_kind: OfferKind::Pocket {
                id: "234".to_string(),
                exchange_rate: ExchangeRate {
                    currency_code: "".to_string(),
                    rate: 0,
                    updated_at: SystemTime::now(),
                },
                topup_value_minor_units: 0,
                topup_value_sats: Some(0),
                exchange_fee_minor_units: 0,
                exchange_fee_rate_permyriad: 0,
                lightning_payout_fee: None,
                error: None,
            },
        };
        let latest_payments = vec![incoming_payment, topup, failed_topup];

        let filtered_topups = filter_out_recently_claimed_topups(topups, latest_payments);

        assert_eq!(filtered_topups.len(), 1);
        assert_eq!(filtered_topups.first().unwrap().id, "234");
    }
}
