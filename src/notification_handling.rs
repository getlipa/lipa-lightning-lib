use crate::async_runtime::AsyncRuntime;
use crate::environment::Environment;
use crate::errors::Result;
use crate::{enable_backtrace, start_sdk, Config};
use breez_sdk_core::{BreezEvent, EventListener};
use perro::{permanent_failure, MapToError};
use serde::Deserialize;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// A notification to be displayed to the user.
#[derive(Debug)]
pub enum Notification {
    /// The notification that a previously issued bolt11 invoice was paid.
    /// The `amount_sat` of the payment is provided.
    ///
    /// The `payment_hash` can be used to directly open the associated [`Payment`](crate::Payment) using
    /// [`LightningNode::get_payment`](crate::LightningNode::get_payment).
    Bolt11PaymentReceived {
        amount_sat: u64,
        payment_hash: String,
    },
}

/// An action to be taken by the consumer of this library upon calling [`handle_notification`].
#[derive(Debug)]
pub enum RecommendedAction {
    None,
    ShowNotification { notification: Notification },
}

/// Handles a notification.
///
/// Notifications are used to wake up the node in order to process some request. Currently supported
/// requests are:
/// * Receive a payment from a previously issued bolt11 invoice.
///
/// The `timeout` is the maximum time this function will wait for the request to be processed.
/// The node start-up time isn't included.
pub fn handle_notification(
    config: Config,
    notification_payload: String,
    timeout: Duration,
) -> Result<RecommendedAction> {
    enable_backtrace();

    let payload: Payload = serde_json::from_str(&notification_payload)
        .map_to_invalid_input("Invalid notification payload")?;

    let rt = AsyncRuntime::new()?;

    let (tx, rx) = mpsc::channel();
    let event_listener = Box::new(NotificationHandlerEventListener { event_sender: tx });
    let environment = Environment::load(config.environment);
    let _sdk = start_sdk(&rt, &config, &environment, event_listener)?;

    match payload {
        Payload::PaymentReceived { payment_hash } => {
            handle_payment_received_notification(rx, payment_hash, timeout)
        }
    }
}

fn handle_payment_received_notification(
    event_receiver: Receiver<BreezEvent>,
    payment_hash: String,
    timeout: Duration,
) -> Result<RecommendedAction> {
    let start = Instant::now();
    while Instant::now().duration_since(start) < timeout {
        let event = match event_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(e) => e,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                permanent_failure!("The SDK stopped running unexpectedly");
            }
        };

        if let BreezEvent::InvoicePaid { details } = event {
            if details.payment_hash == payment_hash {
                return Ok(RecommendedAction::ShowNotification {
                    notification: Notification::Bolt11PaymentReceived {
                        amount_sat: details.payment.map(|p| p.amount_msat).unwrap_or(0) / 1000, // payment will only be None for corrupted GL payments. This is unlikely, so giving an optional amount seems overkill.
                        payment_hash,
                    },
                });
            }
        }
    }

    Ok(RecommendedAction::None)
}

#[derive(Deserialize)]
#[serde(tag = "template", content = "data")]
#[serde(rename_all = "snake_case")]
enum Payload {
    PaymentReceived { payment_hash: String },
}

struct NotificationHandlerEventListener {
    event_sender: Sender<BreezEvent>,
}

impl EventListener for NotificationHandlerEventListener {
    fn on_event(&self, e: BreezEvent) {
        let _ = self.event_sender.send(e);
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_handling::Payload;

    const PAYMENT_RECEIVED_PAYLOAD_JSON: &str = r#"{
                                                 "template": "payment_received",
                                                 "data": {
                                                  "payment_hash": "hash"
                                                 }
                                                }"#;

    #[test]
    pub fn test_payload_deserialize() {
        let payment_received_payload: Payload =
            serde_json::from_str(PAYMENT_RECEIVED_PAYLOAD_JSON).unwrap();
        assert!(matches!(
            payment_received_payload,
            Payload::PaymentReceived {
                payment_hash: hash
            } if hash == "hash"
        ));
    }
}
