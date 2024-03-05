use crate::BreezHealthCheckStatus;

/// Asynchronous events that the consumer of this library might be interested in handling are delivered through this interface.
/// These callbacks will only be called once per event.
pub trait EventsCallback: Send + Sync {
    /// This callback will be called when a payment has been received.
    ///
    /// Parameters:
    /// * `payment_hash` - can be used cross-reference this claimed payment with a previously issued invoice.
    fn payment_received(&self, payment_hash: String);

    /// This callback will be called when a channel has started closing
    /// *WARNING* This will currently never be called as the Breez SDK doesn't support channel closed events. Thus, for now, it can be ignored.
    ///
    /// On the MVP version of lipa wallet, this event is unexpected and is likely to result in funds moving
    /// on-chain, thus becoming unavailable. If this happens, the user should be informed of the problem and that he
    /// should contact lipa.
    ///
    /// Parameters:
    /// * `channel_id` - Channel ID encoded in hexadecimal.
    /// * `reason` - provides a reason for the close
    fn channel_closed(&self, channel_id: String, reason: String);

    /// This callback will be called when a payment has been successfully sent (the payee received the funds)
    ///
    /// Parameters:
    /// * `payment_hash` - the hash of the payment can be used to cross-reference this event to the payment that has succeeded
    /// * `payment_preimage` - the preimage of the payment can be used as proof of payment
    fn payment_sent(&self, payment_hash: String, payment_preimage: String);

    /// This callback will be called when a payment has failed and no further attempts will be pursued.
    ///
    /// Parameters:
    /// * `payment_hash` - the hash of the payment can be used to cross-reference this event to the payment that has failed
    fn payment_failed(&self, payment_hash: String);

    /// This callback will be called when a change to the Breez services health is noticed
    ///
    /// Parameters:
    /// * `status` - the new status
    fn breez_health_status_changed_to(&self, status: BreezHealthCheckStatus);

    /// This callback will be called every time a sync cycle is performed.
    /// It can be used as a trigger to update the balance and activities list.
    fn synced(&self);
}
