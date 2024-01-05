mod print_events_handler;
mod setup;

use crate::setup::start_alice;

use serial_test::file_serial;
use uniffi_lipalightninglib::{InvoiceCreationMetadata, InvoiceDetails, Movement, Payment};

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

    let payment = node.get_payment(invoice.payment_hash.clone()).unwrap();
    assert_invoice_matches_payment(&invoice, &payment);

    let latest_movements = node.get_latest_movements(1).unwrap();
    let movement_from_list = latest_movements.pending_movements.first().unwrap();
    assert_eq!(&Movement::Payment { payment }, movement_from_list);
}

fn assert_invoice_matches_payment(invoice: &InvoiceDetails, payment: &Payment) {
    assert_eq!(invoice, &payment.invoice_details);
    assert_eq!(invoice.payment_hash, payment.hash);
    assert_eq!(invoice.amount.as_ref().unwrap(), &payment.requested_amount);
    assert_eq!(invoice.description, payment.description);
}
