pub trait EventsCallback: Send + Sync {
    fn payment_received(&self, payment_hash: String);

    fn channel_closed(&self, channel_id: String, reason: String);

    fn payment_sent(&self, payment_hash: String, payment_preimage: String);

    fn payment_failed(&self, payment_hash: String);
}
