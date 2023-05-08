mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod sending_payments_test {
    use crate::setup::{mocked_storage_node, setup_outbound_capacity};
    use eel::{InvoiceDetails, LightningNode};
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance::{LspdLnd, NigiriCln, NigiriLnd};
    use crate::wait_for_eq;

    const PAYMENT_AMOUNT: u64 = 1_000_000;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn pay_invoice_direct_peer_test_and_invoice_decoding_test() {
        nigiri::setup_environment_with_lsp();
        let node = mocked_storage_node().start_or_panic();
        invoice_decode_test(&node);
        setup_outbound_capacity(&node);

        // Test vanilla payment
        let invoice = nigiri::issue_invoice(LspdLnd, "test", PAYMENT_AMOUNT, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        node.pay_invoice(invoice, String::new()).unwrap();

        wait_for_eq!(
            nigiri::query_node_balance(LspdLnd).unwrap() - initial_balance,
            PAYMENT_AMOUNT
        );

        // Test a regular payment but using an invoice that has no amount specified
        let invoice =
            nigiri::lnd_issue_invoice(LspdLnd, "open amount invoice", None, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        let payment_result = node.pay_invoice(invoice, String::new());
        assert!(matches!(
            payment_result,
            Err(perro::Error::InvalidInput { .. })
        ));

        // no payment took place
        sleep(Duration::from_secs(2));
        assert_eq!(
            initial_balance,
            nigiri::query_node_balance(LspdLnd).unwrap()
        );

        // Test paying open invoices
        let invoice =
            nigiri::lnd_issue_invoice(LspdLnd, "open amount invoice", None, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        node.pay_open_invoice(invoice, PAYMENT_AMOUNT, String::new())
            .unwrap();

        wait_for_eq!(
            nigiri::query_node_balance(LspdLnd).unwrap() - initial_balance,
            PAYMENT_AMOUNT
        );

        // Test paying open invoices specifying 0 as the payment amount
        let invoice =
            nigiri::lnd_issue_invoice(LspdLnd, "open amount invoice", None, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        let payment_result = node.pay_open_invoice(invoice, 0, String::new());
        assert!(matches!(
            payment_result,
            Err(perro::Error::InvalidInput { .. })
        ));

        // no payment took place
        sleep(Duration::from_secs(2));
        assert_eq!(
            initial_balance,
            nigiri::query_node_balance(LspdLnd).unwrap()
        );

        // Test paying open invoices using an invoice with a specified amount
        let invoice = nigiri::issue_invoice(LspdLnd, "test", PAYMENT_AMOUNT, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        let payment_result = node.pay_open_invoice(invoice, PAYMENT_AMOUNT, String::new());
        assert!(matches!(
            payment_result,
            Err(perro::Error::InvalidInput { .. })
        ));

        // no payment took place
        sleep(Duration::from_secs(2));
        assert_eq!(
            initial_balance,
            nigiri::query_node_balance(LspdLnd).unwrap()
        );
    }

    const THOUSAND_SATS: u64 = 1_000_000;
    const SECONDS_IN_AN_HOUR: u64 = 3600;
    const DESCRIPTION_SAMPLE: &str = "Luke, I Am Your Father";

    fn invoice_decode_test(node: &LightningNode) {
        // Test invoice from CLN
        let invoice = nigiri::issue_invoice(
            NigiriCln,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            Some(THOUSAND_SATS),
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(NigiriCln).unwrap().pub_key,
        );

        // Test invoice from LspdLND
        let invoice = nigiri::issue_invoice(
            LspdLnd,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            Some(THOUSAND_SATS),
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(LspdLnd).unwrap().pub_key,
        );

        // Test invoice from NigiriLND
        let invoice = nigiri::issue_invoice(
            NigiriLnd,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            Some(THOUSAND_SATS),
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(NigiriLnd).unwrap().pub_key,
        );

        // Test open amount invoice (no amount specified)
        let invoice =
            nigiri::lnd_issue_invoice(LspdLnd, DESCRIPTION_SAMPLE, None, SECONDS_IN_AN_HOUR)
                .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            None,
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(LspdLnd).unwrap().pub_key,
        );
    }

    fn assert_invoice_details(
        invoice_details: InvoiceDetails,
        amount_msat: Option<u64>,
        description: &str,
        expiry_time: Duration,
        payee_pub_key: &str,
    ) {
        assert_eq!(invoice_details.amount_msat, amount_msat);
        assert_eq!(invoice_details.description, description);
        assert_eq!(invoice_details.expiry_interval, expiry_time);
        assert_eq!(invoice_details.payee_pub_key, payee_pub_key);
    }
}
