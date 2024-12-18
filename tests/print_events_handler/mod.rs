use uniffi_lipalightninglib::{BreezHealthCheckStatus, EventsCallback};

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

    fn swap_received(&self, payment_hash: String) {
        println!("A swap has been received! Its hash is {payment_hash}");
    }

    fn reverse_swap_sent(&self, reverse_swap_id: String) {
        println!("A reverse swap has been sent! Its id is {reverse_swap_id}");
    }

    fn reverse_swap_settled(&self, reverse_swap_id: String) {
        println!("A reverse swap has been settled! Its id is {reverse_swap_id}");
    }

    fn reverse_swap_cancelled(&self, reverse_swap_id: String) {
        println!("A reverse swap has been cancelled! Its id is {reverse_swap_id}");
    }

    fn breez_health_status_changed_to(&self, status: BreezHealthCheckStatus) {
        println!("The Breez SDK health status changed to {status:?}");
    }

    fn synced(&self) {}
}
