use eel::interfaces::EventHandler;
use log::info;

pub struct PrintEventsHandler {}

impl EventHandler for PrintEventsHandler {
    fn payment_received(&self, payment_hash: String) {
        info!("Received a payment with hash {payment_hash}");
    }

    fn payment_sent(&self, payment_hash: String, payment_preimage: String) {
        info!("A payment has been successfully sent! Its preimage is {payment_preimage}, the hash is {payment_hash}");
    }

    fn payment_failed(&self, payment_hash: String) {
        info!("An outgoing payment has failed! Its hash is {payment_hash}");
    }

    fn channel_closed(&self, channel_id: String, reason: String) {
        info!("A channel was closed! Channel ID {channel_id} was closed due to {reason}");
    }
}
