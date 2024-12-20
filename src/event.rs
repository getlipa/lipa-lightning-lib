use crate::{analytics::AnalyticsInterceptor, EventsCallback};

use breez_sdk_core::{BreezEvent, EventListener, PaymentDetails, ReverseSwapStatus, SwapStatus};
use std::sync::Arc;

pub(crate) struct LipaEventListener {
    events_callback: Arc<Box<dyn EventsCallback>>,
    analytics_interceptor: Arc<AnalyticsInterceptor>,
}

impl LipaEventListener {
    pub fn new(
        events_callback: Arc<Box<dyn EventsCallback>>,
        analytics_interceptor: Arc<AnalyticsInterceptor>,
    ) -> Self {
        Self {
            events_callback,
            analytics_interceptor,
        }
    }
}

impl EventListener for LipaEventListener {
    fn on_event(&self, e: BreezEvent) {
        report_event_for_analytics(&e, &self.analytics_interceptor);
        match e {
            BreezEvent::NewBlock { .. } => {}
            BreezEvent::InvoicePaid { details } => {
                self.events_callback.payment_received(details.payment_hash)
            }
            BreezEvent::Synced => {
                self.events_callback.synced();
            }
            BreezEvent::PaymentSucceed { details } => {
                if let PaymentDetails::Ln { data } = details.details {
                    self.events_callback
                        .payment_sent(data.payment_hash, data.payment_preimage)
                }
            }
            BreezEvent::PaymentFailed { details } => {
                if let Some(invoice) = details.invoice {
                    self.events_callback.payment_failed(invoice.payment_hash)
                }
            }
            BreezEvent::BackupStarted => {}
            BreezEvent::BackupSucceeded => {}
            BreezEvent::BackupFailed { .. } => {}
            BreezEvent::SwapUpdated { details } => {
                if details.status == SwapStatus::Completed {
                    self.events_callback
                        .swap_received(hex::encode(details.payment_hash));
                }
            }
            BreezEvent::ReverseSwapUpdated { details } => match details.status {
                ReverseSwapStatus::CompletedSeen => {
                    self.events_callback.reverse_swap_sent(details.id);
                }
                ReverseSwapStatus::CompletedConfirmed => {
                    self.events_callback.reverse_swap_settled(details.id);
                }
                ReverseSwapStatus::Cancelled => {
                    self.events_callback.reverse_swap_cancelled(details.id);
                }
                _ => {}
            },
        }
    }
}

pub(crate) fn report_event_for_analytics(
    e: &BreezEvent,
    analytics_interceptor: &AnalyticsInterceptor,
) {
    match e {
        BreezEvent::NewBlock { .. } => {}
        BreezEvent::InvoicePaid { details } => {
            analytics_interceptor.request_succeeded(details.clone());
        }
        BreezEvent::Synced => {}
        BreezEvent::PaymentSucceed { details } => {
            if let PaymentDetails::Ln { .. } = details.details.clone() {
                analytics_interceptor.pay_succeeded(details.clone());
            }
        }
        BreezEvent::PaymentFailed { details } => {
            if details.invoice.is_some() {
                analytics_interceptor.pay_failed(details.clone());
            }
        }
        BreezEvent::BackupStarted => {}
        BreezEvent::BackupSucceeded => {}
        BreezEvent::BackupFailed { .. } => {}
        BreezEvent::SwapUpdated { .. } => {}
        BreezEvent::ReverseSwapUpdated { .. } => {}
    }
}
