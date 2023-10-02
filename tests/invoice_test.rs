mod print_events_handler;
mod setup;

use crate::setup::start_alice;

use uniffi_lipalightninglib::DecodeInvoiceError;

use serial_test::file_serial;

#[test]
#[file_serial(key, "/tmp/3l-int-tests-lock")]
fn test_invoice() {
    let node = start_alice().unwrap();

    let invalid_invoice = "invalid".to_string();
    let result = node.decode_invoice(invalid_invoice);
    assert!(matches!(result, Err(DecodeInvoiceError::ParseError { .. })));

    // TODO: Implement when it is implemented and released
    //       https://github.com/breez/breez-sdk/issues/462
    // let testnet_invoice = "lntb10u1pjs6ugjpp5erx7rjnr3gr0c4f8qxznsnxe7rfhwe2nuenl6hvv77gxdf8cu8asdqqcqzzsxqyz5vqsp5a36x2elfn26a3aucfushvnka5x7vr74nyck5cetfaxe4gshj2z6q9qyyssqc0fe55lksskte20k0zf92kepcpe5q5a7g42mye8le7hryrsvnjmpqdvknufx522h9zcvq8dgwl5wm0vca0uevkqtfmv8ygk2z7wualqpgsvmp9".to_string();
    // let result = node.decode_invoice(testnet_invoice);
    // assert!(matches!(
    //     result,
    //     Err(DecodeInvoiceError::NetworkMismatch {
    //         expected: Network::Bitcoin,
    //         found: Network::Testnet
    //     })
    // ));

    let bitcoi_address = "1DTHjgRiPnCYhgy7PcKxEEWAyFi4VoJpqi".to_string();
    let result = node.decode_invoice(bitcoi_address);
    assert!(matches!(
        result,
        Err(DecodeInvoiceError::SemanticError { .. })
    ));

    let own_invoice = node
        .create_invoice(100_000, None, String::new(), String::new())
        .unwrap();
    let result = node.decode_invoice(own_invoice.invoice);
    assert!(matches!(result, Err(DecodeInvoiceError::PayingToSelf)));

    let expired_invoice = "lnbc1pjs6m8ppp5krf0wqz805p6v2f2ducge75lxg5v9dk34t3vdamz4j0h9ycstp6sdqu2askcmr9wssx7e3q2dshgmmndp5scqzzsxqyz5vqsp5hymglgtm35e7hy6w7c4wswmcs77xg0hu8ns83wmkfskq9p34w8ds9qyyssq389370f0wm48ecajj9nz5vnx2nuru2cwmkdz93qywy45uvf5f7sjp9wjuv3gyvtr8emm6w56s7x94fpxqkgfpgeqq38xz85k9clnkqcq3rw49n".to_string();
    let result = node.decode_invoice(expired_invoice);
    assert!(matches!(result, Err(DecodeInvoiceError::InvoiceExpired)));

    // TODO: Generate a valid invoice from another node and check it.
}
