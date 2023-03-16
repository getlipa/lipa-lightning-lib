pub trait EventsCallback: Send + Sync {
    fn payment_received(&self, payment_hash: String, amount_msat: u64);

    fn channel_closed(&self, channel_id: String, reason: String);

    fn payment_sent(&self, payment_hash: String, payment_preimage: String, fee_paid_msat: u64);

    fn payment_failed(&self, payment_hash: String);
}
