use eel::errors::LipaResult;
use eel::interfaces::EventHandler;
use log::info;

pub struct PrintEventsHandler {}

impl EventHandler for PrintEventsHandler {
    fn payment_received(&self, payment_hash: String, amount_msat: u64) -> LipaResult<()> {
        info!(
            "Received a payment! Value of {} milli satoshis and payment hash is {}",
            amount_msat, payment_hash
        );
        Ok(())
    }

    fn channel_closed(&self, channel_id: String, reason: String) -> LipaResult<()> {
        info!(
            "A channel was closed! Channel ID {} was closed due to {}",
            channel_id, reason
        );
        Ok(())
    }

    fn payment_sent(
        &self,
        payment_hash: String,
        payment_preimage: String,
        fee_paid_msat: u64,
    ) -> LipaResult<()> {
        info!(
            "A payment has been successfully sent! Its preimage is {}, the hash is {}, and a total of {} msats were paid in lightning fees", 
            payment_preimage,
            payment_hash,
            fee_paid_msat
        );
        Ok(())
    }

    fn payment_failed(&self, payment_hash: String) -> LipaResult<()> {
        info!("A payment has failed! Its hash is {}", payment_hash);
        Ok(())
    }
}
