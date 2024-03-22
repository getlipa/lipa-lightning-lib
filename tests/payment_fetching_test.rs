mod print_events_handler;
mod setup;

use crate::setup::start_alice;

use serial_test::file_serial;
use uniffi_lipalightninglib::{Activity, InvoiceCreationMetadata};

#[test]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_payment_fetching() {
    let node = start_alice().unwrap();

    let invoice = node
        .create_invoice(
            100_000,
            None,
            "description".into(),
            InvoiceCreationMetadata {
                request_currency: "EUR".into(),
            },
        )
        .unwrap();

    let payment = node
        .get_incoming_payment(invoice.payment_hash.clone())
        .unwrap();
    assert_eq!(invoice, payment.payment_info.invoice_details);
    assert_eq!(invoice.payment_hash, payment.payment_info.hash);
    assert_eq!(invoice.amount.as_ref().unwrap(), &payment.requested_amount);
    assert_eq!(invoice.description, payment.payment_info.description);

    let latest_activities = node.get_latest_activities(1).unwrap();
    let activity_from_list = latest_activities.pending_activities.first().unwrap();
    assert_eq!(
        &Activity::IncomingPayment {
            incoming_payment_info: payment
        },
        activity_from_list
    );
}
