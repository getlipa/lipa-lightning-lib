mod print_events_handler;
mod setup;

use crate::setup::start_alice;

use serial_test::file_serial;
use uniffi_lipalightninglib::{InvoiceCreationMetadata, InvoiceDetails, Payment};

#[test]
#[file_serial(key, "/tmp/3l-int-tests-lock")]
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

    let latest_payments = node.get_latest_payments(1).unwrap();
    let payment_from_list = latest_payments.pending_payments.first().unwrap();
    assert_eq!(&payment, payment_from_list);
}

fn assert_invoice_matches_payment(invoice: &InvoiceDetails, payment: &Payment) {
    assert_eq!(invoice, &payment.invoice_details);
    assert_eq!(invoice.payment_hash, payment.hash);
    assert_eq!(invoice.amount.as_ref().unwrap(), &payment.requested_amount);
    assert_eq!(invoice.description, payment.description);
}
