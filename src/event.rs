use crate::{analytics::AnalyticsInterceptor, EventsCallback};

use breez_sdk_core::{BreezEvent, EventListener, PaymentDetails};
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
        match e {
            BreezEvent::NewBlock { .. } => {}
            BreezEvent::InvoicePaid { details } => {
                self.analytics_interceptor
                    .request_succeeded(details.clone());
                self.events_callback.payment_received(details.payment_hash)
            }
            BreezEvent::Synced => {}
            BreezEvent::PaymentSucceed { details } => {
                if let PaymentDetails::Ln { data } = details.details.clone() {
                    self.analytics_interceptor.pay_succeeded(details);
                    self.events_callback
                        .payment_sent(data.payment_hash, data.payment_preimage)
                }
            }
            BreezEvent::PaymentFailed { details } => {
                if let Some(invoice) = details.invoice.clone() {
                    self.analytics_interceptor.pay_failed(details);
                    self.events_callback.payment_failed(invoice.payment_hash)
                }
            }
            BreezEvent::BackupStarted => {}
            BreezEvent::BackupSucceeded => {}
            BreezEvent::BackupFailed { .. } => {}
        }
    }
}
