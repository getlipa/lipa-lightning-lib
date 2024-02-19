use crate::async_runtime::AsyncRuntime;
use crate::environment::Environment;
use crate::errors::Result;
use crate::{enable_backtrace, Config, RuntimeErrorCode, EXEMPT_FEE, MAX_FEE_PERMYRIAD};
use breez_sdk_core::{
    BreezEvent, BreezServices, EventListener, GreenlightCredentials, GreenlightNodeConfig,
    NodeConfig,
};
use perro::{permanent_failure, MapToError};
use serde::Deserialize;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// A notification to be displayed to the user.
#[derive(Debug)]
pub enum Notification {
    /// The notification that a previously issued bolt11 invoice was paid.
    /// The `amount_sat` of the payment is provided.
    ///
    /// The `hash` can be used to directly open the associated [`Payment`](crate::Payment) using
    /// [`LightningNode::get_payment`](crate::LightningNode::get_payment).
    Bolt11PaymentReceived { amount_sat: u64, hash: String },
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

    let (tx, rx) = mpsc::channel();

    let rt = AsyncRuntime::new()?;

    let _sdk = start_sdk(&rt, &config, tx)?;

    let payload: Payload = serde_json::from_str(&notification_payload)
        .map_to_invalid_input("Invalid notification payload")?;

    match payload {
        Payload::PaymentReceived { payment_hash } => {
            handle_payment_received_notification(rx, payment_hash, timeout)
        }
        Payload::AddressTxsConfirmed { .. } => {
            // TODO: implement handling of AddressTxsConfirmed
            Ok(RecommendedAction::None)
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
            Err(RecvTimeoutError::Disconnected) => {
                permanent_failure!("The SDK stopped running unexpectedly");
            }
            Err(_) => continue,
        };

        if let BreezEvent::InvoicePaid { details } = event {
            if details.payment_hash == payment_hash {
                return Ok(RecommendedAction::ShowNotification {
                    notification: Notification::Bolt11PaymentReceived {
                        amount_sat: details.payment.map(|p| p.amount_msat).unwrap_or(0) / 1000, // payment will only be None for corrupted GL payments. This is unlikely, so giving an optional amount seems overkill.
                        hash: payment_hash,
                    },
                });
            }
        }
    }

    Ok(RecommendedAction::None)
}

fn start_sdk(
    rt: &AsyncRuntime,
    config: &Config,
    event_sender: Sender<BreezEvent>,
) -> Result<Arc<BreezServices>> {
    let environment = Environment::load(config.environment);

    let device_cert = env!("BREEZ_SDK_PARTNER_CERTIFICATE").as_bytes().to_vec();
    let device_key = env!("BREEZ_SDK_PARTNER_KEY").as_bytes().to_vec();
    let partner_credentials = GreenlightCredentials {
        device_cert,
        device_key,
    };

    let mut breez_config = BreezServices::default_config(
        environment.environment_type.clone(),
        env!("BREEZ_SDK_API_KEY").to_string(),
        NodeConfig::Greenlight {
            config: GreenlightNodeConfig {
                partner_credentials: Some(partner_credentials),
                invite_code: None,
            },
        },
    );

    breez_config.working_dir = config.local_persistence_path.clone();
    breez_config.exemptfee_msat = EXEMPT_FEE.msats;
    breez_config.maxfee_percent = MAX_FEE_PERMYRIAD as f64 / 100_f64;

    let event_listener = Box::new(NotificationHandlerEventListener { event_sender });

    rt.handle()
        .block_on(BreezServices::connect(
            breez_config,
            config.seed.clone(),
            event_listener,
        ))
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to initialize a breez sdk instance",
        )
}

#[derive(Deserialize)]
#[serde(tag = "template", content = "data")]
#[serde(rename_all = "snake_case")]
enum Payload {
    PaymentReceived { payment_hash: String },
    AddressTxsConfirmed { address: String },
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

    const ADDRESS_TXS_CONFIRMED_PAYLOAD_JSON: &str =
        r#"{"template": "address_txs_confirmed", "data": {"address": "bc1"}}"#;

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

        let address_txs_confirmed_payload: Payload =
            serde_json::from_str(ADDRESS_TXS_CONFIRMED_PAYLOAD_JSON).unwrap();
        assert!(matches!(
            address_txs_confirmed_payload,
            Payload::AddressTxsConfirmed {
                address
            } if address == "bc1"
        ));
    }
}
