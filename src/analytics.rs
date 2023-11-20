use crate::amount::AsSats;
use crate::async_runtime::Handle;
use crate::errors::Result;
use crate::key_derivation::derive_analytics_key;
use crate::locker::Locker;
use crate::util::{unix_timestamp_to_system_time, LogIgnoreError};
use crate::{ExchangeRate, InvoiceDetails, UserPreferences};
use breez_sdk_core::{
    InvoicePaidDetails, Payment, PaymentDetails, PaymentFailedData, ReceivePaymentResponse,
};
use log::{error, info, Level};
use parrot::{AnalyticsClient, AnalyticsEvent, PayFailureReason, PaymentSource};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use uuid::Uuid;

pub(crate) struct AnalyticsInterceptor {
    pub analytics_client: Arc<AnalyticsClient>,
    pub user_preferences: Arc<Mutex<UserPreferences>>,
    pub rt_handle: Handle,
}

/// Includes metadata about a payment the client received in any way.
pub struct PaymentMetadata {
    pub source: PaymentSource,
    /// The current time the client started the pay process (e.g. scanned the invoice)
    pub process_started_at: SystemTime,
}

/// Includes metadata about an invoice the user created.
pub struct InvoiceCreationMetadata {
    /// The currency the user used to define the requested amount e.g. chf/sat
    pub request_currency: String,
}

impl AnalyticsInterceptor {
    pub fn new(
        analytics_client: Arc<AnalyticsClient>,
        user_preferences: Arc<Mutex<UserPreferences>>,
        rt_handle: Handle,
    ) -> Self {
        Self {
            analytics_client,
            user_preferences,
            rt_handle,
        }
    }

    pub fn pay_initiated(
        &self,
        invoice_details: InvoiceDetails,
        metadata: PaymentMetadata,
        paid_amount: Option<u64>,
        exchange_rate: Option<ExchangeRate>,
    ) {
        let invoice_amount = invoice_details.amount.map(|a| a.sats.as_sats().msats);
        let paid_amount_msat = match paid_amount {
            None => match invoice_amount {
                None => {
                    error!(
                        "Couldn't retrieve invoice amount of initiated payment: {}",
                        invoice_details.payment_hash
                    );
                    return;
                }
                Some(a) => a,
            },
            Some(a) => a,
        };

        let user_currency = self.user_preferences.lock_unwrap().fiat_currency.clone();
        let analytics_client = Arc::clone(&self.analytics_client);

        self.rt_handle.spawn(async move {
            analytics_client
                .report_event(AnalyticsEvent::PayInitiated {
                    payment_hash: invoice_details.payment_hash,
                    paid_amount_msat,
                    requested_amount_msat: invoice_amount,
                    sats_per_user_currency: exchange_rate.map(|e| e.rate),
                    source: metadata.source,
                    user_currency,
                    process_started_at: metadata.process_started_at,
                    executed_at: SystemTime::now(),
                })
                .await
                .log_ignore_error(Level::Warn, "Failed to report an analytics event")
        });
    }

    pub fn pay_succeeded(&self, payment: Payment) {
        if let PaymentDetails::Ln { data } = payment.details {
            let analytics_client = Arc::clone(&self.analytics_client);

            self.rt_handle.spawn(async move {
                analytics_client
                    .report_event(AnalyticsEvent::PaySucceeded {
                        payment_hash: data.payment_hash,
                        ln_fees_paid_msat: payment.fee_msat,
                        confirmed_at: unix_timestamp_to_system_time(
                            payment.payment_time.unsigned_abs(),
                        ),
                    })
                    .await
                    .log_ignore_error(Level::Warn, "Failed to report an analytics event")
            });
        }
    }

    pub fn pay_failed(&self, failed_data: PaymentFailedData) {
        if failed_data.invoice.is_none() {
            info!("Payment failed without invoice, not reporting");
            return;
        }

        let analytics_client = Arc::clone(&self.analytics_client);

        self.rt_handle.spawn(async move {
            analytics_client
                .report_event(AnalyticsEvent::PayFailed {
                    payment_hash: failed_data.invoice.unwrap().payment_hash,
                    reason: map_error_to_failure_reason(failed_data.error),
                    failed_at: SystemTime::now(),
                })
                .await
                .log_ignore_error(Level::Warn, "Failed to report an analytics event")
        });
    }

    pub fn request_initiated(
        &self,
        receive_response: ReceivePaymentResponse,
        exchange_rate: Option<ExchangeRate>,
        metadata: InvoiceCreationMetadata,
    ) {
        let analytics_client = Arc::clone(&self.analytics_client);
        let user_currency = self.user_preferences.lock_unwrap().fiat_currency.clone();

        self.rt_handle.spawn(async move {
            analytics_client
                .report_event(AnalyticsEvent::RequestInitiated {
                    payment_hash: receive_response.ln_invoice.payment_hash,
                    entered_amount_msat: receive_response.ln_invoice.amount_msat,
                    sats_per_user_currency: exchange_rate.map(|e| e.rate),
                    user_currency,
                    request_currency: metadata.request_currency,
                    created_at: SystemTime::now(),
                })
                .await
                .log_ignore_error(Level::Warn, "Failed to report an analytics event")
        });
    }

    // TODO complete data https://github.com/breez/breez-sdk/pull/593
    pub fn request_succeeded(&self, paid_details: InvoicePaidDetails) {
        let analytics_client = Arc::clone(&self.analytics_client);

        self.rt_handle.spawn(async move {
            analytics_client
                .report_event(AnalyticsEvent::RequestSucceeded {
                    payment_hash: paid_details.payment_hash,
                    paid_amount_sat: 0,
                    channel_opening_fee_msat: 0,
                    received_at: SystemTime::now(),
                })
                .await
                .log_ignore_error(Level::Warn, "Failed to report an analytics event")
        });
    }
}

pub(crate) fn derive_analytics_keys(seed: &[u8; 64]) -> Result<String> {
    let key = derive_analytics_key(seed)?;
    Ok(Uuid::new_v5(&Uuid::NAMESPACE_OID, &key)
        .hyphenated()
        .to_string())
}

fn map_error_to_failure_reason(error: String) -> PayFailureReason {
    if error.starts_with("Route not found:") {
        return PayFailureReason::NoRoute;
    }

    PayFailureReason::Unknown
}
