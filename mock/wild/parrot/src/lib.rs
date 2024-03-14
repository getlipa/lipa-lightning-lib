use honey_badger::asynchronous::Auth;
use std::sync::Arc;

pub use parrot::{AnalyticsEvent, PayFailureReason, PaymentSource};

pub struct AnalyticsClient {}

impl AnalyticsClient {
    pub fn new(_backend_url: String, _analytics_id: String, _auth: Arc<Auth>) -> Self {
        Self {}
    }

    pub async fn report_event(&self, _analytics_event: AnalyticsEvent) -> graphql::Result<()> {
        Ok(())
    }
}
