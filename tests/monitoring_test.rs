mod print_events_handler;
mod setup;

use crate::setup::{start_specific_node, Environment, NodeType};
use std::fs::OpenOptions;
use uniffi_lipalightninglib::{
    Activity, BreezHealthCheckStatus, EventsCallback, InvoiceCreationMetadata, LightningNode,
    PaymentMetadata, PaymentState,
};

use anyhow::Result;
use parrot::PaymentSource;
use serial_test::file_serial;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const PAYMENT_AMOUNT_SATS: u64 = 300;
const MAX_PAYMENT_TIME_SECS: u64 = 60;
const INVOICE_DESCRIPTION: &str = "automated bolt11 test";

const TIME_RESULTS_FILE_NAME: &str = "test_times.json";

struct TransactingNode {
    node: LightningNode,
    sent_payment_receiver: Receiver<String>,
    received_payment_receiver: Receiver<String>,
}

struct ReturnFundsEventsHandler {
    pub received_payment_sender: Sender<String>,
    pub sent_payment_sender: Sender<String>,
}

struct PaymentAmount {
    exact: u64,
    plus_fees: u64,
    minus_fees: u64,
}

impl EventsCallback for ReturnFundsEventsHandler {
    fn payment_received(&self, payment_hash: String) {
        self.received_payment_sender.send(payment_hash).unwrap();
    }

    fn channel_closed(&self, channel_id: String, reason: String) {
        panic!("A channel was closed! Channel ID {channel_id} was closed due to {reason}");
    }

    fn payment_sent(&self, payment_hash: String, _: String) {
        self.sent_payment_sender.send(payment_hash).unwrap();
    }

    fn payment_failed(&self, payment_hash: String) {
        panic!("An outgoing payment has failed! Its hash is {payment_hash}");
    }

    fn swap_received(&self, _payment_hash: String) {
        // do nothing
    }

    fn breez_health_status_changed_to(&self, _status: BreezHealthCheckStatus) {
        // do nothing
    }

    fn synced(&self) {
        // do nothing
    }
}

fn append_to_file(file_path: &str, content: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)?;

    writeln!(file, "{}", content)?;

    Ok(())
}

#[test]
#[ignore]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn node_can_start() {
    let start = Instant::now();
    setup_node(NodeType::Sender).unwrap();
    let elapsed = start.elapsed();
    append_to_file(
        TIME_RESULTS_FILE_NAME,
        &format!(
            "{{ \"test\": \"start_node\", \"time_seconds\": \"{}\" }}",
            elapsed.as_secs_f64()
        ),
    )
    .unwrap()
}

#[test]
#[ignore]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn lsp_fee_can_be_fetched() {
    let sender = setup_node(NodeType::Sender).unwrap();
    sender.node.query_lsp_fee().unwrap();
}

#[test]
#[ignore]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn exchange_rate_can_be_fetched_and_is_recent() {
    let sender = setup_node(NodeType::Sender).unwrap();
    let rate = sender.node.get_exchange_rate().unwrap();
    // Check exchange rate is recent
    let backend_exchange_rate_update_interval_secs: u64 = 5 * 60;
    assert!(
        rate.updated_at.elapsed().unwrap().as_secs() <= backend_exchange_rate_update_interval_secs
    );
}

#[test]
#[ignore]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn invoice_can_be_created() {
    let sender = setup_node(NodeType::Sender).unwrap();
    let start = Instant::now();
    sender
        .node
        .create_invoice(
            10000,
            None,
            INVOICE_DESCRIPTION.to_string(),
            InvoiceCreationMetadata {
                request_currency: "EUR".to_string(),
            },
        )
        .unwrap();
    let elapsed = start.elapsed();
    append_to_file(
        TIME_RESULTS_FILE_NAME,
        &format!(
            "{{ \"test\": \"create_invoice\", \"time_seconds\": \"{}\" }}",
            elapsed.as_secs_f64()
        ),
    )
    .unwrap()
}

#[test]
#[ignore]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn payments_can_be_listed() {
    let sender = setup_node(NodeType::Sender).unwrap();
    sender.node.get_latest_activities(2).unwrap();
}

#[test]
#[ignore = "This test costs real sats!"]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn payments_can_be_performed() {
    let amount = get_payment_amount();

    let sender = setup_node(NodeType::Sender).unwrap();
    assert!(node_has_enough_outbound(&sender, amount.plus_fees).unwrap());

    let receiver = setup_node(NodeType::Receiver).unwrap();
    assert!(node_has_enough_inbound(&sender, amount.plus_fees).unwrap());

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

    let payment_hash = send_invoice.payment_hash.clone();

    let start = Instant::now();
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
        &sender.sent_payment_receiver,
        &receiver.received_payment_receiver,
    )
    .unwrap();
    let elapsed = start.elapsed();
    append_to_file(
        TIME_RESULTS_FILE_NAME,
        &format!(
            "{{ \"test\": \"send_payment\", \"time_seconds\": \"{}\" }}",
            elapsed.as_secs_f64()
        ),
    )
    .unwrap();

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
        &receiver.sent_payment_receiver,
        &sender.received_payment_receiver,
    )
    .unwrap();

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
                    outgoing_payment_info
                        .payment_info
                        .invoice_details
                        .payment_hash,
                    send_invoice.payment_hash
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
                    incoming_payment_info
                        .payment_info
                        .invoice_details
                        .payment_hash,
                    return_invoice.payment_hash
                );
            }
            _ => {
                panic!("Unexpected activity: {payment:?}");
            }
        }
    }
}

fn node_has_enough_outbound(
    transacting_node: &TransactingNode,
    min_outbound_sats: u64,
) -> Result<bool> {
    let node_info = transacting_node.node.get_node_info()?;
    let outbound_capacity = node_info.channels_info.outbound_capacity.sats;
    Ok(outbound_capacity > min_outbound_sats)
}

fn node_has_enough_inbound(
    transacting_node: &TransactingNode,
    min_inbound_sats: u64,
) -> Result<bool> {
    let node_info = transacting_node.node.get_node_info()?;
    let inbound_capacity = node_info.channels_info.total_inbound_capacity.sats;
    Ok(inbound_capacity > min_inbound_sats)
}

fn setup_node(node_type: NodeType) -> Result<TransactingNode> {
    let (sent_payment_inform, sent_payment_learn) = channel();
    let (received_payment_inform, received_payment_learn) = channel();

    let node = start_specific_node(
        Some(node_type.clone()),
        Box::new(ReturnFundsEventsHandler {
            sent_payment_sender: sent_payment_inform,
            received_payment_sender: received_payment_inform,
        }),
        false,
        Environment::Stage,
    )?;

    Ok(TransactingNode {
        node,
        sent_payment_receiver: sent_payment_learn,
        received_payment_receiver: received_payment_learn,
    })
}

fn wait_for_payment(
    payment_hash: &str,
    sender: &Receiver<String>,
    receiver: &Receiver<String>,
) -> Result<(), &'static str> {
    let start_time = Instant::now();
    let mut sender_sent_payment = false;
    let mut receiver_received_payment = false;
    loop {
        if start_time.elapsed().as_secs() >= MAX_PAYMENT_TIME_SECS {
            return Err("Payment did not go through within {MAX_PAYMENT_TIME_SECS} seconds!");
        }

        if let Ok(received_payment_hash) = sender.recv_timeout(Duration::from_secs(1)) {
            if received_payment_hash == payment_hash {
                sender_sent_payment = true;
            } else {
                return Err("Received unexpected payment");
            }
        }

        if let Ok(received_payment_hash) = receiver.recv_timeout(Duration::from_secs(1)) {
            if received_payment_hash == payment_hash {
                receiver_received_payment = true;
            } else {
                return Err("Received unexpected payment");
            }
        }

        if sender_sent_payment && receiver_received_payment {
            return Ok(());
        }
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