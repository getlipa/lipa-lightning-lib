use crate::amount::AsSats;
use crate::analytics::{derive_analytics_keys, AnalyticsInterceptor};
use crate::async_runtime::AsyncRuntime;
use crate::auth::{build_async_auth, build_auth};
use crate::data_store::DataStore;
use crate::errors::{NotificationHandlingErrorCode, NotificationHandlingResult};
use crate::event::report_event_for_analytics;
use crate::exchange_rate_provider::{ExchangeRateProvider, ExchangeRateProviderImpl};
use crate::logger::init_logger_once;
use crate::util::LogIgnoreError;
use crate::{
    enable_backtrace, register_webhook_url, sanitize_input, start_sdk, EnableStatus,
    LightningNodeConfig, RuntimeErrorCode, UserPreferences, DB_FILENAME, LOGS_DIR,
};
use breez_sdk_core::{
    BreezEvent, BreezServices, EventListener, OpenChannelFeeRequest, Payment, PaymentStatus,
    ReceivePaymentRequest, SwapInfo,
};
use log::{debug, Level};
use parrot::AnalyticsClient;
use perro::{ensure, invalid_input, permanent_failure, runtime_error, MapToError, ResultTrait};
use pigeon::submit_lnurl_pay_invoice;
use serde::Deserialize;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

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
    OnchainPaymentSwappedIn {
        amount_sat: u64,
        payment_hash: String,
    },
    /// The notification that an on-chain pay transaction has been broadcast successfully.
    // TODO: Return `payment_hash` and `amount_sat`. Requires changes by the Breez SDK https://github.com/breez/breez-sdk-greenlight/issues/1159
    OnchainPaymentSwappedOut {},
    /// The notification that an invoice was created and submitted for payment as part of an
    /// incoming LNURL payment.
    /// The `amount_sat` of the created invoice is provided.
    LnurlInvoiceCreated { amount_sat: u64 },
}

/// A configuration struct used to enable/disable processing of different payloads in [`handle_notification`].
pub struct NotificationToggles {
    pub payment_received_is_enabled: bool,
    pub address_txs_confirmed_is_enabled: bool,
    pub lnurl_pay_request_is_enabled: bool,
}

/// Handles a notification.
///
/// Notifications are used to wake up the node in order to process some request. Currently supported
/// requests are:
/// * Receive a payment from a previously issued bolt11 invoice.
/// * Receive a payment from a confirmed swap.
/// * Issue an invoice in order to receive an LNURL payment.
///
/// Requires network: **yes**
pub fn handle_notification(
    config: LightningNodeConfig,
    notification_payload: String,
    notification_toggles: NotificationToggles,
    timeout: Duration,
) -> NotificationHandlingResult<Notification> {
    enable_backtrace();
    if let Some(level) = config.file_logging_level {
        init_logger_once(
            level,
            &Path::new(&config.local_persistence_path).join(LOGS_DIR),
        )
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;
    }
    debug!("Started handling a notification with payload: {notification_payload}");

    let timeout_instant = Instant::now() + timeout;

    let payload = match serde_json::from_str::<Payload>(&notification_payload) {
        Ok(p) => p,
        Err(e) => {
            invalid_input!("The provided payload was not recognized. Error: {e} - JSON Payload: {notification_payload}")
        }
    };

    match payload {
        Payload::PaymentReceived { .. } => ensure!(
            notification_toggles.payment_received_is_enabled,
            runtime_error(
                NotificationHandlingErrorCode::NotificationDisabledInNotificationToggles,
                "PaymentReceived notification dismissed due to disabled setting in NotificationToggles"
            )
        ),
        Payload::AddressTxsConfirmed { .. } => ensure!(
            notification_toggles.address_txs_confirmed_is_enabled,
            runtime_error(
                NotificationHandlingErrorCode::NotificationDisabledInNotificationToggles,
                "AddressTxsConfirmed notification dismissed due to disabled setting in NotificationToggles"
            )
        ),
        Payload::LnurlPayRequest { .. } => ensure!(
            notification_toggles.lnurl_pay_request_is_enabled,
            runtime_error(
                NotificationHandlingErrorCode::NotificationDisabledInNotificationToggles,
                "LnurlPayRequest notification dismissed due to disabled setting in NotificationToggles"
            )
        ),
    }

    let rt = AsyncRuntime::new()
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;

    let (tx, rx) = mpsc::channel();
    let analytics_interceptor = build_analytics_interceptor(&config, &rt)?;
    let event_listener = Box::new(NotificationHandlerEventListener::new(
        tx,
        analytics_interceptor,
    ));
    let sdk = rt
        .handle()
        .block_on(start_sdk(&config, event_listener))
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;

    match payload {
        Payload::PaymentReceived { payment_hash } => {
            handle_payment_received_notification(rt, sdk, rx, payment_hash, timeout_instant)
        }
        Payload::AddressTxsConfirmed { address } => {
            handle_address_txs_confirmed_notification(rt, sdk, rx, address, timeout_instant)
        }
        Payload::LnurlPayRequest { data } => {
            handle_lnurl_pay_request_notification(rt, sdk, config, data)
        }
    }
}

fn build_analytics_interceptor(
    config: &LightningNodeConfig,
    rt: &AsyncRuntime,
) -> NotificationHandlingResult<AnalyticsInterceptor> {
    let db_path = format!("{}/{DB_FILENAME}", config.local_persistence_path);
    let data_store = DataStore::new(&db_path)
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;

    let fiat_currency = data_store
        .retrieve_last_set_fiat_currency()
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?
        .ok_or(permanent_failure(
            "No fiat currency set. Node must be started before handling notifications",
        ))?;
    let user_preferences = Arc::new(Mutex::new(UserPreferences {
        fiat_currency,
        timezone_config: config.timezone_config.clone(),
    }));

    let strong_typed_seed = get_strong_typed_seed(config)?;
    let async_auth = Arc::new(
        build_async_auth(
            &strong_typed_seed,
            &config.remote_services_config.backend_url,
        )
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?,
    );

    let analytics_client = AnalyticsClient::new(
        config.remote_services_config.backend_url.clone(),
        derive_analytics_keys(&strong_typed_seed)
            .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?,
        Arc::clone(&async_auth),
    );

    let analytics_config = data_store
        .retrieve_analytics_config()
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;
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
    timeout_instant: Instant,
) -> NotificationHandlingResult<Notification> {
    let payment = wait_for_payment(rt, sdk, event_receiver, &payment_hash, timeout_instant)?;
    Ok(Notification::Bolt11PaymentReceived {
        amount_sat: payment.amount_msat / 1000,
        payment_hash,
    })
}

fn handle_swap_notification(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    event_receiver: Receiver<BreezEvent>,
    address: String,
    in_progress_swap: SwapInfo,
    timeout_instant: Instant,
) -> NotificationHandlingResult<Notification> {
    ensure!(
        in_progress_swap.bitcoin_address == address,
        runtime_error(
            NotificationHandlingErrorCode::InProgressSwapNotFound,
            "Received an address_txs_confirmed event for an address different from the \
            current in-progress swap address"
        )
    );

    rt.handle()
        .block_on(sdk.redeem_swap(address.clone()))
        .map_to_runtime_error(
            NotificationHandlingErrorCode::NodeUnavailable,
            "Failed to start a swap redeem",
        )?;

    let payment_hash = hex::encode(in_progress_swap.payment_hash);
    let payment = wait_for_payment(rt, sdk, event_receiver, &payment_hash, timeout_instant)?;
    Ok(Notification::OnchainPaymentSwappedIn {
        amount_sat: payment.amount_msat / 1000,
        payment_hash,
    })
}

fn handle_reverse_swap_notification(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    address: String,
) -> NotificationHandlingResult<Notification> {
    debug!(
        "Trying to claim reverse swap with lock address: {}",
        address
    );

    rt.handle()
        .block_on(sdk.claim_reverse_swap(address))
        .map_to_runtime_error(
            NotificationHandlingErrorCode::NodeUnavailable,
            "Failed to claim reverse swap",
        )?;

    Ok(Notification::OnchainPaymentSwappedOut {})
}

fn handle_address_txs_confirmed_notification(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    event_receiver: Receiver<BreezEvent>,
    address: String,
    timeout_instant: Instant,
) -> NotificationHandlingResult<Notification> {
    if let Some(in_progress_swap) = rt
        .handle()
        .block_on(sdk.in_progress_swap())
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to get in-progress swap",
        )
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?
    {
        return handle_swap_notification(
            rt,
            sdk,
            event_receiver,
            address,
            in_progress_swap,
            timeout_instant,
        );
    }

    handle_reverse_swap_notification(rt, sdk, address)
}

fn handle_lnurl_pay_request_notification(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    config: LightningNodeConfig,
    data: LnurlPayRequestData,
) -> NotificationHandlingResult<Notification> {
    // Prevent payments that need a new channel from being received
    let open_channel_fee_response = rt
        .handle()
        .block_on(sdk.open_channel_fee(OpenChannelFeeRequest {
            amount_msat: Some(data.amount_msat),
            expiry: None,
        }))
        .map_to_runtime_error(
            NotificationHandlingErrorCode::NodeUnavailable,
            "Failed to query open channel fees",
        )?;

    // Prevent payments sent to disabled address from being received
    let db_path = format!("{}/{DB_FILENAME}", config.local_persistence_path);
    let mut data_store = DataStore::new(&db_path)
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;
    match data_store
        .retrieve_lightning_addresses()
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?
        .iter()
        .find(|(a, _)| data.recipient == *a)
    {
        None => {
            permanent_failure!(
                "Received LNURL Pay request notification for unrecognized address/phone number"
            )
        }
        Some((_, EnableStatus::FeatureDisabled)) => {
            permanent_failure!(
                "Received LNURL Pay request notification for disabled address/phone number feature"
            )
        }
        Some((_, EnableStatus::Enabled)) => {}
    }

    let strong_typed_seed = get_strong_typed_seed(&config)?;

    if let Some(fee_msat) = open_channel_fee_response.fee_msat {
        if fee_msat > 0 {
            report_insuficcient_inbound_liquidity(
                rt,
                &config.remote_services_config.backend_url,
                &strong_typed_seed,
                &data.id,
            )?;
            runtime_error!(
                NotificationHandlingErrorCode::InsufficientInboundLiquidity,
                "Rejecting an inbound LNURL-pay payment because of insufficient inbound liquidity"
            );
        }
    }

    let auth = build_auth(
        &strong_typed_seed,
        &config.remote_services_config.backend_url,
    )
    .map_to_runtime_error(
        NotificationHandlingErrorCode::LipaServiceUnavailable,
        "Failed to authenticate against backend",
    )?;

    // Register webhook in case user hasn't started the wallet for a long time
    //  (Breez expires webhook registrations)
    register_webhook_url(&rt, &sdk, &auth, &config)
        .map_runtime_error_to(NotificationHandlingErrorCode::NodeUnavailable)?;

    // Create invoice
    let receive_payment_result = rt
        .handle()
        .block_on(sdk.receive_payment(ReceivePaymentRequest {
            amount_msat: data.amount_msat,
            description: String::new(),
            preimage: None,
            opening_fee_params: None,
            use_description_hash: None,
            expiry: None,
            cltv: None,
        }))
        .map_to_runtime_error(
            NotificationHandlingErrorCode::NodeUnavailable,
            "Failed to create invoice",
        )?;
    if receive_payment_result.opening_fee_msat.is_some() {
        report_insuficcient_inbound_liquidity(
            rt,
            &config.remote_services_config.backend_url,
            &strong_typed_seed,
            &data.id,
        )?;
        runtime_error!(
            NotificationHandlingErrorCode::InsufficientInboundLiquidity,
            "Rejecting an inbound LNURL-pay payment because of insufficient inbound liquidity"
        )
    }

    // Invoice is not persisted in invoices table because we are not interested in unpaid invoices
    // resulting from incoming LNURL payments

    let fiat_currency = data_store
        .retrieve_last_set_fiat_currency()
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?
        .ok_or(permanent_failure(
            "No fiat currency set. Node must be started before handling notifications",
        ))?;
    // Store payment info (exchange rates, user preferences, etc...)
    let user_preferences = UserPreferences {
        fiat_currency,
        timezone_config: config.timezone_config.clone(),
    };
    let exchange_rate_provider = ExchangeRateProviderImpl::new(
        config.remote_services_config.backend_url.clone(),
        Arc::new(auth),
    );
    let exchange_rates = exchange_rate_provider
        .query_all_exchange_rates()
        .map_to_runtime_error(
            NotificationHandlingErrorCode::LipaServiceUnavailable,
            "Failed to get exchange rates",
        )?;

    data_store
        .store_payment_info(
            &receive_payment_result.ln_invoice.payment_hash,
            user_preferences,
            exchange_rates,
            None,
            Some(data.recipient),
            data.payer_comment,
        )
        .log_ignore_error(Level::Error, "Failed to persist payment info");

    // Submit created invoice to backend
    let async_auth = build_async_auth(
        &strong_typed_seed,
        &config.remote_services_config.backend_url,
    )
    .map_to_runtime_error(
        NotificationHandlingErrorCode::LipaServiceUnavailable,
        "Failed to authenticate against backend",
    )?;
    rt.handle()
        .block_on(submit_lnurl_pay_invoice(
            &config.remote_services_config.backend_url,
            &async_auth,
            data.id,
            Some(receive_payment_result.ln_invoice.bolt11),
        ))
        .map_runtime_error_to(NotificationHandlingErrorCode::LipaServiceUnavailable)?;

    Ok(Notification::LnurlInvoiceCreated {
        amount_sat: data.amount_msat.as_msats().sats_round_down().sats,
    })
}

fn report_insuficcient_inbound_liquidity(
    rt: AsyncRuntime,
    backend_url: &str,
    strong_typed_seed: &[u8; 64],
    id: &str,
) -> NotificationHandlingResult<()> {
    let async_auth = build_async_auth(strong_typed_seed, backend_url).map_to_runtime_error(
        NotificationHandlingErrorCode::LipaServiceUnavailable,
        "Failed to authenticate against backend",
    )?;
    rt.handle()
        .block_on(submit_lnurl_pay_invoice(
            backend_url,
            &async_auth,
            id.to_string(),
            None,
        ))
        .map_runtime_error_to(NotificationHandlingErrorCode::LipaServiceUnavailable)
}

fn get_confirmed_payment(
    rt: &AsyncRuntime,
    sdk: &Arc<BreezServices>,
    payment_hash: &str,
) -> NotificationHandlingResult<Option<Payment>> {
    let payment = rt
        .handle()
        .block_on(sdk.payment_by_hash(payment_hash.to_string()))
        .map_to_runtime_error(
            RuntimeErrorCode::NodeUnavailable,
            "Failed to get payment by hash",
        )
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)?;
    if let Some(payment) = payment {
        if payment.status == PaymentStatus::Complete {
            return Ok(Some(payment));
        }
    }
    Ok(None)
}

fn wait_for_payment(
    rt: AsyncRuntime,
    sdk: Arc<BreezServices>,
    event_receiver: Receiver<BreezEvent>,
    payment_hash: &str,
    timeout_instant: Instant,
) -> NotificationHandlingResult<Payment> {
    while Instant::now() < timeout_instant {
        debug!("Checking existent payments...");
        if let Some(payment) = get_confirmed_payment(&rt, &sdk, payment_hash)? {
            debug!("Checking existent payments... Found");
            return Ok(payment);
        }
        debug!("Checking existent payments... None");

        if let Some(payment) = check_for_received_payment(&event_receiver, payment_hash)? {
            return Ok(payment);
        }
    }

    runtime_error!(
        NotificationHandlingErrorCode::ExpectedPaymentNotReceived,
        "Expected incoming payment with hash {payment_hash} but it was not received"
    )
}

fn check_for_received_payment(
    event_receiver: &Receiver<BreezEvent>,
    payment_hash: &str,
) -> NotificationHandlingResult<Option<Payment>> {
    debug!("Waiting for payment to be received...");
    match event_receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(BreezEvent::InvoicePaid { details }) if details.payment_hash == payment_hash => {
            debug!("Waiting for payment to be received... Received");
            debug!("Waiting for synced event...");
            // We want to wait as long as possible to decrease the likelihood of
            // the signer being shut down while HTLCs are still in-flight.
            wait_for_synced_event(event_receiver)?;
            debug!("Waiting for synced event... Synced");
            Ok(details.payment)
        }
        Ok(_) | Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => {
            permanent_failure!("The SDK stopped running unexpectedly");
        }
    }
}

/// Wait for synced event without timeout.
fn wait_for_synced_event(event_receiver: &Receiver<BreezEvent>) -> NotificationHandlingResult<()> {
    loop {
        match event_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(BreezEvent::Synced) => return Ok(()),
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                permanent_failure!("The SDK stopped running unexpectedly");
            }
        }
    }
}

fn get_strong_typed_seed(config: &LightningNodeConfig) -> NotificationHandlingResult<[u8; 64]> {
    sanitize_input::strong_type_seed(&config.seed)
        .map_runtime_error_using(NotificationHandlingErrorCode::from_runtime_error)
}

#[derive(Deserialize)]
#[serde(tag = "template", content = "data")]
#[serde(rename_all = "snake_case")]
enum Payload {
    PaymentReceived {
        payment_hash: String,
    },
    AddressTxsConfirmed {
        address: String,
    },
    LnurlPayRequest {
        #[serde(flatten)]
        data: LnurlPayRequestData,
    },
}

#[derive(Deserialize)]
struct LnurlPayRequestData {
    amount_msat: u64,
    recipient: String,
    payer_comment: Option<String>,
    id: String,
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

    const LNURL_PAY_REQUEST_PAYLOAD_JSON: &str = r#"{
                                                 "template": "lnurl_pay_request",
                                                 "data": {
                                                  "amount_msat": 12345,
                                                  "recipient": "recipient",
                                                  "payer_comment": "payer_comment",
                                                  "id": "id"
                                                 }
                                                }"#;

    const LNURL_PAY_REQUEST_WITHOUT_COMMENT_PAYLOAD_JSON: &str = r#"{
                                                 "template": "lnurl_pay_request",
                                                 "data": {
                                                  "amount_msat": 12345,
                                                  "recipient": "recipient",
                                                  "payer_comment": null,
                                                  "id": "id"
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

        let lnurl_pay_request_payload: Payload =
            serde_json::from_str(LNURL_PAY_REQUEST_PAYLOAD_JSON).unwrap();
        assert!(matches!(
            lnurl_pay_request_payload,
            Payload::LnurlPayRequest {
                data
            } if data.amount_msat == 12345 && data.recipient == "recipient" && data.payer_comment == Some("payer_comment".to_string()) && data.id == "id"
        ));

        let lnurl_pay_request_without_comment_payload: Payload =
            serde_json::from_str(LNURL_PAY_REQUEST_WITHOUT_COMMENT_PAYLOAD_JSON).unwrap();
        assert!(matches!(
            lnurl_pay_request_without_comment_payload,
            Payload::LnurlPayRequest {
                data
            } if data.amount_msat == 12345 && data.recipient == "recipient" && data.payer_comment.is_none() && data.id == "id"
        ));
    }
}
