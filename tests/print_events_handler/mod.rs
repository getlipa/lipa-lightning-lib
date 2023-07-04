use uniffi_lipalightninglib::EventsCallback;

pub struct PrintEventsHandler {}

impl EventsCallback for PrintEventsHandler {
    fn payment_received(&self, payment_hash: String) {
        println!("Received a payment with hash {payment_hash}");
    }

    fn channel_closed(&self, channel_id: String, reason: String) {
        println!("A channel was closed! Channel ID {channel_id} was closed due to {reason}");
    }

    fn payment_sent(&self, payment_hash: String, payment_preimage: String) {
        println!("A payment has been successfully sent! Its preimage is {payment_preimage}, the hash is {payment_hash}");
    }

    fn payment_failed(&self, payment_hash: String) {
        println!("An outgoing payment has failed! Its hash is {payment_hash}");
    }
}
