use log::info;
use uniffi_lipalightninglib::EventsCallback;

pub struct PrintEventsHandler {}

impl EventsCallback for PrintEventsHandler {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) {
        info!(
            "Received a payment! Value of {} milli satoshis and payment hash is {}",
            amount_msat, payment_hash
        );
    }

    fn channel_closed(&self, channel_id: String, reason: String) {
        info!(
            "A channel was closed! Channel ID {} was closed due to {}",
            channel_id, reason
        );
    }

    fn payment_sent(&self, payment_hash: String, payment_preimage: String, fee_paid_msat: u64) {
        info!(
            "A payment has been successfully sent! Its preimage is {}, the hash is {}, and a total of {} msats were paid in lightning fees",
            payment_preimage,
            payment_hash,
            fee_paid_msat
        );
    }

    fn payment_failed(&self, payment_hash: String) {
        info!("A payment has failed! Its hash is {}", payment_hash);
    }
}
