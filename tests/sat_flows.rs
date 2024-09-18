mod print_events_handler;
mod setup;

use crate::setup::{start_specific_node, NodeType};
use uniffi_lipalightninglib::{
    Activity, BreezHealthCheckStatus, EventsCallback, InvoiceCreationMetadata, LightningNode,
    PaymentMetadata, PaymentState,
};

use parrot::PaymentSource;
use serial_test::file_serial;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thousands::Separable;

const PAYMENT_AMOUNT_SATS: u64 = 300;
const MAX_PAYMENT_TIME_SECS: u64 = 60;
const INVOICE_DESCRIPTION: &str = "automated bolt11 test";

struct TransactingNode {
    node: LightningNode,
    received_payments: Arc<Mutex<Vec<String>>>,
    sent_payments: Arc<Mutex<Vec<String>>>,
}

struct ReturnFundsEventsHandler {
    pub received_payments: Arc<Mutex<Vec<String>>>,
    pub sent_payments: Arc<Mutex<Vec<String>>>,
}

struct PaymentAmount {
    exact: u64,
    plus_fees: u64,
    minus_fees: u64,
}

impl EventsCallback for ReturnFundsEventsHandler {
    fn payment_received(&self, payment_hash: String) {
        self.received_payments.lock().unwrap().push(payment_hash);
    }

    fn channel_closed(&self, channel_id: String, reason: String) {
        panic!("A channel was closed! Channel ID {channel_id} was closed due to {reason}");
    }

    fn payment_sent(&self, payment_hash: String, _: String) {
        self.sent_payments.lock().unwrap().push(payment_hash);
    }

    fn payment_failed(&self, payment_hash: String) {
        panic!("An outgoing payment has failed! Its hash is {payment_hash}");
    }

    fn swap_received(&self, _payment_hash: String) {
        todo!()
    }

    fn breez_health_status_changed_to(&self, _status: BreezHealthCheckStatus) {
        // do nothing
    }

    fn synced(&self) {
        // do nothing
    }
}

#[test]
#[ignore = "This test costs real sats!"]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_bolt11_payment() {
    let amount = get_payment_amount();

    let sender = setup_sender_node(amount.plus_fees);
    let receiver = setup_receiver_node(amount.plus_fees);

    let before_invoice_creation = Instant::now();
    let send_invoice = receiver
        .node
        .create_invoice(
            amount.exact,
            None,
            INVOICE_DESCRIPTION.to_string(),
            InvoiceCreationMetadata {
                request_currency: "EUR".to_string(),
            },
        )
        .unwrap();
    println!(
        "Created invoice in {} milliseconds",
        before_invoice_creation
            .elapsed()
            .as_millis()
            .separate_with_commas()
    );
    let payment_hash = send_invoice.payment_hash.clone();

    let before_paying_invoice = Instant::now();
    sender
        .node
        .pay_invoice(
            send_invoice.clone(),
            PaymentMetadata {
                source: PaymentSource::Manual,
                process_started_at: std::time::SystemTime::now(),
            },
        )
        .unwrap();

    wait_for_payment(
        &payment_hash,
        &sender.sent_payments,
        &receiver.received_payments,
    )
    .unwrap();
    println!(
        "Payment [{} sat] successful after {} milliseconds",
        amount.exact,
        before_paying_invoice
            .elapsed()
            .as_millis()
            .separate_with_commas()
    );

    // return funds to keep sender well funded
    let return_invoice = sender
        .node
        .create_invoice(
            amount.minus_fees,
            None,
            INVOICE_DESCRIPTION.to_string(),
            InvoiceCreationMetadata {
                request_currency: "EUR".to_string(),
            },
        )
        .unwrap();
    let payment_hash = return_invoice.payment_hash.clone();

    receiver
        .node
        .pay_invoice(
            return_invoice.clone(),
            PaymentMetadata {
                source: PaymentSource::Manual,
                process_started_at: std::time::SystemTime::now(),
            },
        )
        .unwrap();

    wait_for_payment(
        &payment_hash,
        &receiver.sent_payments,
        &sender.received_payments,
    )
    .unwrap();

    // list payments
    let payments = sender.node.get_latest_activities(2).unwrap();
    assert_eq!(payments.completed_activities.len(), 2);

    for payment in payments.completed_activities {
        match payment {
            Activity::OutgoingPayment {
                outgoing_payment_info,
            } => {
                assert_eq!(
                    outgoing_payment_info.payment_info.payment_state,
                    PaymentState::Succeeded
                );
                assert_eq!(
                    outgoing_payment_info.payment_info.invoice_details,
                    send_invoice.clone()
                );
            }
            Activity::IncomingPayment {
                incoming_payment_info,
            } => {
                assert_eq!(
                    incoming_payment_info.requested_amount.sats,
                    amount.minus_fees
                );
                assert_eq!(
                    incoming_payment_info.payment_info.payment_state,
                    PaymentState::Succeeded
                );
                assert_eq!(
                    incoming_payment_info.payment_info.invoice_details,
                    return_invoice.clone()
                );
            }
            _ => {
                panic!("Unexpected activity: {payment:?}");
            }
        }
    }

    // Check whether exchange rate has updated during test run
    let backend_exchange_rate_update_interval_secs: u64 = 5 * 60; // exchange_rate.updated_at measures the elpased time since the server updated, NOT since 3L last fetched that data.
    let exchange_rate = sender.node.get_exchange_rate().unwrap();
    assert!(
        exchange_rate.updated_at.elapsed().unwrap().as_secs()
            <= backend_exchange_rate_update_interval_secs
    );
}

fn setup_sender_node(payment_amount_plus_fees: u64) -> TransactingNode {
    let tn = setup_node(Some(NodeType::Sender));

    let node_info = tn.node.get_node_info().unwrap();
    let outbound_capacity = node_info.channels_info.outbound_capacity.sats;
    assert!(
        outbound_capacity > payment_amount_plus_fees,
        "Sending node ({}) is insufficiently funded [Outbound capacity: {outbound_capacity}, required: {payment_amount_plus_fees}]",
        node_info.node_pubkey
    );

    tn
}

fn setup_receiver_node(max_payment_amount: u64) -> TransactingNode {
    let tn = setup_node(Some(NodeType::Receiver));

    let node_info = tn.node.get_node_info().unwrap();
    let inbound_capacity = node_info.channels_info.inbound_capacity.sats;
    assert!(
        inbound_capacity > max_payment_amount,
        "Sending node ({}) has insufficient inbound capacity: {inbound_capacity} (required: {max_payment_amount})",
        node_info.node_pubkey
    );

    tn
}

fn setup_node(node_type: Option<NodeType>) -> TransactingNode {
    let received_payments = Arc::new(Mutex::new(Vec::<String>::new()));
    let sent_payments = Arc::new(Mutex::new(Vec::<String>::new()));

    let before_node_started = Instant::now();
    let node = start_specific_node(
        node_type.clone(),
        Box::new(ReturnFundsEventsHandler {
            received_payments: received_payments.clone(),
            sent_payments: sent_payments.clone(),
        }),
    )
    .unwrap();
    println!(
        "{:?} node started in {} milliseconds",
        node_type.unwrap(),
        before_node_started
            .elapsed()
            .as_millis()
            .separate_with_commas()
    );

    // Additional check: Have LSP fees been fetched successfully
    assert!(node.query_lsp_fee().is_ok());

    TransactingNode {
        node,
        received_payments,
        sent_payments,
    }
}

fn wait_for_payment(
    payment_hash: &str,
    sender: &Arc<Mutex<Vec<String>>>,
    receiver: &Arc<Mutex<Vec<String>>>,
) -> Result<(), &'static str> {
    let start_time = Instant::now();
    let mut sender_sent_payment = false;
    let mut receiver_received_payment = false;
    loop {
        if start_time.elapsed().as_secs() >= MAX_PAYMENT_TIME_SECS {
            return Err("Payment did not go through within {MAX_PAYMENT_TIME_SECS} seconds!");
        }

        if let Some(last_payment_hash) = sender.lock().unwrap().last() {
            if last_payment_hash == payment_hash {
                sender_sent_payment = true;
            } else {
                return Err("Unexpected payment sent: {last_payment_hash} (expected payment: {payment_hash})");
            }
        }

        if let Some(last_payment_hash) = receiver.lock().unwrap().last() {
            if last_payment_hash == payment_hash {
                receiver_received_payment = true;
            } else {
                return Err("Unexpected payment received: {last_payment_hash} (expected payment: {payment_hash})");
            }
        }

        if sender_sent_payment && receiver_received_payment {
            return Ok(());
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn get_payment_amount() -> PaymentAmount {
    let fee_deviation = 5 + PAYMENT_AMOUNT_SATS / 25;

    PaymentAmount {
        exact: PAYMENT_AMOUNT_SATS,
        plus_fees: PAYMENT_AMOUNT_SATS + fee_deviation,
        minus_fees: PAYMENT_AMOUNT_SATS - fee_deviation,
    }
}
