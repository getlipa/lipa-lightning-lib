use crate::analytics::{derive_analytics_keys, AnalyticsInterceptor};
use crate::async_runtime::AsyncRuntime;
use crate::auth::build_async_auth;
use crate::data_store::DataStore;
use crate::environment::Environment;
use crate::errors::Result;
use crate::event::report_event_for_analytics;
use crate::logger::init_logger_once;
use crate::{
    enable_backtrace, sanitize_input, start_sdk, Config, RuntimeErrorCode, UserPreferences,
    DB_FILENAME, LOGS_DIR,
};
use breez_sdk_core::{
    BreezEvent, BreezServices, EventListener, InvoicePaidDetails, Payment, PaymentStatus,
};
use log::{debug, error};
use parrot::AnalyticsClient;
use perro::{invalid_input, permanent_failure, MapToError};
use serde::Deserialize;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(60);

/// A notification to be displayed to the user.
#[derive(Debug)]
pub enum Notification {
    /// The notification that a previously issued bolt11 invoice was paid.
    /// The `amount_sat` of the payment is provided.
    ///
    /// The `payment_hash` can be used to directly open the associated [`IncomingPaymentInfo`](crate::IncomingPaymentInfo) or
    /// [`OutgoingPaymentInfo`](crate::OutgoingPaymentInfo) using
    /// [`LightningNode::get_incoming_payment`](crate::LightningNode::get_incoming_payment) or
    /// [`LightningNode::get_outgoing_payment`](crate::LightningNode::get_outgoing_payment).
    Bolt11PaymentReceived {
        amount_sat: u64,
        payment_hash: String,
    },
    /// The notification that an onchain receive has completed successfully.
    /// The `amount_sat` of the payment is provided.
    ///
    /// The `payment_hash` can be used to directly open the associated
    /// [`Activity`](crate::Activity) using
    /// [`LightningNode::get_activity`](crate::LightningNode::get_activity).
    OnchainPaymentReceived {
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
pub fn handle_notification(
    config: Config,
    notification_payload: String,
) -> Result<RecommendedAction> {
    enable_backtrace();
    if let Some(level) = config.file_logging_level {
        init_logger_once(
            level,
            &Path::new(&config.local_persistence_path).join(LOGS_DIR),
        )?;
    }
    debug!("Started handling a notification.");

    let payload = match serde_json::from_str::<Payload>(&notification_payload) {
        Ok(p) => p,
        Err(e) => {
            error!("Notification payload not recognized. Error: {e} - JSON Payload: {notification_payload}");
            return Ok(RecommendedAction::None);
        }
    };

    let rt = AsyncRuntime::new()?;

    let (tx, rx) = mpsc::channel();
    let analytics_interceptor = build_analytics_interceptor(&config, &rt)?;
    let event_listener = Box::new(NotificationHandlerEventListener::new(
        tx,
        analytics_interceptor,
    ));
    let environment = Environment::load(config.environment)?;
    let sdk = rt
        .handle()
        .block_on(start_sdk(&config, &environment, event_listener))?;

    match payload {
        Payload::PaymentReceived { payment_hash } => {
            handle_payment_received_notification(rt, sdk, rx, payment_hash)
        }
        Payload::AddressTxsConfirmed { address } => {
            handle_address_txs_confirmed_notification(rt, sdk, rx, address)
        }
    }
}

fn build_analytics_interceptor(config: &Config, rt: &AsyncRuntime) -> Result<AnalyticsInterceptor> {
    let user_preferences = Arc::new(Mutex::new(UserPreferences {
        fiat_currency: config.fiat_currency.clone(),
        timezone_config: config.timezone_config.clone(),
    }));

    let environment = Environment::load(config.environment)?;
    let strong_typed_seed = sanitize_input::strong_type_seed(&config.seed)?;
    let async_auth = Arc::new(build_async_auth(
        &strong_typed_seed,
        environment.backend_url.clone(),
    )?);

    let analytics_client = AnalyticsClient::new(
        environment.backend_url.clone(),
        derive_analytics_keys(&strong_typed_seed)?,
        Arc::clone(&async_auth),
    );

    let db_path = format!("{}/{DB_FILENAME}", config.local_persistence_path);
    let data_store = DataStore::new(&db_path)?;
    let analytics_config = data_store.retrieve_analytics_config()?;
    Ok(AnalyticsInterceptor::new(
        analytics_client,
        Arc::clone(&user_preferences),
        rt.handle(),
        analytics_config,
    ))
}

fn handle_payment_received_notification(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    event_receiver: Receiver<BreezEvent>,
    payment_hash: String,
) -> Result<RecommendedAction> {
    // Check if the payment was already received
    if let Some(payment) = get_confirmed_payment(&rt, &sdk, &payment_hash)? {
        return Ok(RecommendedAction::ShowNotification {
            notification: Notification::Bolt11PaymentReceived {
                amount_sat: payment.amount_msat / 1000,
                payment_hash,
            },
        });
    }

    // Wait for payment to be received
    if let Some(details) = wait_for_payment_with_timeout(event_receiver, &payment_hash)? {
        return Ok(RecommendedAction::ShowNotification {
            notification: Notification::Bolt11PaymentReceived {
                amount_sat: details.payment.map(|p| p.amount_msat).unwrap_or(0) / 1000, // payment will only be None for corrupted GL payments. This is unlikely, so giving an optional amount seems overkill.
                payment_hash,
            },
        });
    }

    Ok(RecommendedAction::None)
}

fn handle_address_txs_confirmed_notification(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    event_receiver: Receiver<BreezEvent>,
    address: String,
) -> Result<RecommendedAction> {
    let in_progress_swap = match rt
        .handle()
        .block_on(sdk.in_progress_swap())
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to get in-progress swap",
        )? {
        None => {
            invalid_input!("Received an address_txs_confirmed event when no swap is in progress");
        }
        Some(s) => s,
    };

    if in_progress_swap.bitcoin_address != address {
        invalid_input!("Received an address_txs_confirmed event for an address different from the current in-progress swap address");
    }

    rt.handle()
        .block_on(sdk.redeem_swap(address.clone()))
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to start a swap redeem",
        )?;

    // Check if the payment was already received
    let payment_hash = hex::encode(in_progress_swap.payment_hash);
    if let Some(payment) = get_confirmed_payment(&rt, &sdk, &payment_hash)? {
        return Ok(RecommendedAction::ShowNotification {
            notification: Notification::OnchainPaymentReceived {
                amount_sat: payment.amount_msat / 1000,
                payment_hash,
            },
        });
    }

    // Wait for payment to arrive
    if let Some(details) = wait_for_payment_with_timeout(event_receiver, &payment_hash)? {
        return Ok(RecommendedAction::ShowNotification {
            notification: Notification::OnchainPaymentReceived {
                amount_sat: details.payment.map(|p| p.amount_msat).unwrap_or(0) / 1000, // payment will only be None for corrupted GL payments. This is unlikely, so giving an optional amount seems overkill.
                payment_hash,
            },
        });
    }

    Ok(RecommendedAction::None)
}

fn get_confirmed_payment(
    rt: &AsyncRuntime,
    sdk: &Arc<BreezServices>,
    payment_hash: &str,
) -> Result<Option<Payment>> {
    let payment = rt
        .handle()
        .block_on(sdk.payment_by_hash(payment_hash.to_string()))
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to get payment by hash",
        )?;
    if let Some(payment) = payment {
        if payment.status == PaymentStatus::Complete {
            return Ok(Some(payment));
        }
    }
    Ok(None)
}

fn wait_for_payment_with_timeout(
    event_receiver: Receiver<BreezEvent>,
    payment_hash: &str,
) -> Result<Option<InvoicePaidDetails>> {
    let start = Instant::now();
    while Instant::now().duration_since(start) < TIMEOUT {
        let event = match event_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(e) => e,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                permanent_failure!("The SDK stopped running unexpectedly");
            }
        };

        if let BreezEvent::InvoicePaid { details } = event {
            if details.payment_hash == payment_hash {
                return Ok(Some(details));
            }
        }
    }
    Ok(None)
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
    analytics_interceptor: AnalyticsInterceptor,
}

impl NotificationHandlerEventListener {
    fn new(event_sender: Sender<BreezEvent>, analytics_interceptor: AnalyticsInterceptor) -> Self {
        NotificationHandlerEventListener {
            event_sender,
            analytics_interceptor,
        }
    }
}

impl EventListener for NotificationHandlerEventListener {
    fn on_event(&self, e: BreezEvent) {
        report_event_for_analytics(&e, &self.analytics_interceptor);
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

    const ADDRESS_TXS_CONFIRMED_PAYLOAD_JSON: &str = r#"{
                                                 "template": "address_txs_confirmed",
                                                 "data": {
                                                  "address": "address"
                                                 }
                                                }"#;

    #[test]
    fn test_payload_deserialize() {
        let payment_received_payload: Payload =
            serde_json::from_str(PAYMENT_RECEIVED_PAYLOAD_JSON).unwrap();
        assert!(matches!(
            payment_received_payload,
            Payload::PaymentReceived {
                payment_hash
            } if payment_hash == "hash"
        ));

        let address_txs_confirmed_payload: Payload =
            serde_json::from_str(ADDRESS_TXS_CONFIRMED_PAYLOAD_JSON).unwrap();
        assert!(matches!(
            address_txs_confirmed_payload,
            Payload::AddressTxsConfirmed {
                address
            } if address == "address"
        ));
    }
}
