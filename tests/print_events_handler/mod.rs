use log::info;
use uniffi_lipalightninglib::callbacks::EventsCallback;
use uniffi_lipalightninglib::errors::CallbackResult;

pub struct PrintEventsHandler {}

impl EventsCallback for PrintEventsHandler {
    fn payment_claimed(&self, payment_hash: String, amount_msat: u64) -> CallbackResult<()> {
        info!(
            "Claimed a payment! Value of {} milli satoshis and payment hash is {}",
            amount_msat, payment_hash
        );
        Ok(())
    }
}
