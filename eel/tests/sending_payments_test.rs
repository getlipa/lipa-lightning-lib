mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod sending_payments_test {
    use eel::invoice::DecodeInvoiceError;
    use eel::LightningNode;
    use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescription, Description};
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{mocked_storage_node, setup_outbound_capacity};
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance::{LspdLnd, NigiriCln, NigiriLnd};
    use crate::{wait_for, wait_for_eq};

    const SECONDS_IN_AN_HOUR: u64 = 3600;
    const DESCRIPTION_SAMPLE: &str = "Luke, I Am Your Father";

    const THOUSAND_SATS: u64 = 1_000_000;
    const FIFE_K_SATS: u64 = 5_000_000;
    const FOURTY_K_SATS: u64 = 40_000_000;
    const NINE_HUNDRED_K_SATS: u64 = 900_000_000;
    const ONE_M_SATS: u64 = 1_000_000_000;
    const MORE_THAN_ONE_M_SATS: u64 = 1_500_000_000;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn pay_invoice_direct_peer_test_and_invoice_decoding_test() {
        nigiri::setup_environment_with_lsp();
        let node = mocked_storage_node().start_or_panic();
        invoice_decode_test(&node);
        setup_outbound_capacity(&node);

        // Test vanilla payment
        let invoice = nigiri::issue_invoice(LspdLnd, "test", FIFE_K_SATS, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        node.pay_invoice(invoice, String::new()).unwrap();

        wait_for_eq!(
            nigiri::query_node_balance(LspdLnd).unwrap() - initial_balance,
            FIFE_K_SATS
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

        node.pay_open_invoice(invoice, FIFE_K_SATS, String::new())
            .unwrap();

        wait_for_eq!(
            nigiri::query_node_balance(LspdLnd).unwrap() - initial_balance,
            FIFE_K_SATS
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
        let invoice = nigiri::issue_invoice(LspdLnd, "test", FIFE_K_SATS, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        let payment_result = node.pay_open_invoice(invoice, FIFE_K_SATS, String::new());
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

        // test sending mpp
        let channels_info = node.get_node_info().channels_info;
        assert_eq!(channels_info.num_usable_channels, 1);
        assert_eq!(channels_info.local_balance_msat, FOURTY_K_SATS);
        assert_eq!(channels_info.total_channel_capacities_msat, ONE_M_SATS);

        let invoice = node
            .create_invoice(NINE_HUNDRED_K_SATS, "test".to_string(), String::new())
            .unwrap()
            .to_string();
        assert!(invoice.starts_with("lnbc"));

        nigiri::pay_invoice(LspdLnd, &invoice).unwrap();
        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            NINE_HUNDRED_K_SATS + FOURTY_K_SATS
        );

        nigiri::initiate_channel_from_remote(node.get_node_info().node_pubkey, LspdLnd);

        wait_for_eq!(node.get_node_info().channels_info.num_channels, 2);
        wait_for_eq!(
            node.get_node_info()
                .channels_info
                .total_channel_capacities_msat,
            ONE_M_SATS * 2
        );

        let invoice = node
            .create_invoice(NINE_HUNDRED_K_SATS, "test".to_string(), String::new())
            .unwrap()
            .to_string();
        assert!(invoice.starts_with("lnbc"));

        wait_for!(nigiri::pay_invoice(LspdLnd, &invoice).is_ok());
        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            NINE_HUNDRED_K_SATS + NINE_HUNDRED_K_SATS + FOURTY_K_SATS
        );

        // Node has 2 channels of 1M SAT each. Paying 1.5M SAT requires sending through both of them
        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();
        let invoice = nigiri::issue_invoice(LspdLnd, "MPP", MORE_THAN_ONE_M_SATS, 3600).unwrap();

        node.pay_invoice(invoice, String::new()).unwrap();

        wait_for_eq!(
            nigiri::query_node_balance(LspdLnd).unwrap(),
            initial_balance + MORE_THAN_ONE_M_SATS
        );
    }

    const MAINNET_INVOICE: &str = "lnbc1m1pj8m78dpp5zumxkd5hhc54rrjhhg8whcp67shph50gkln8hlwnar77rrljq0aqdqvtfz5y32yg4zscqzzsxqzjcsp5tn4l5acc5fwtdq5kz966uyhyqsf9vlj8quekfuf2wrz2u9k762js9qyyssqp5rjsuewp6lrldclvjqpt8gx7a0mk76qhypug40vpzg5cr72cdghcpcxh3t8pyfr7t6l9n4u97d8zupcnwte9vys660wcjcevktxm0cpstydgt";

    fn invoice_decode_test(node: &LightningNode) {
        // Test invoice from CLN
        let invoice = nigiri::issue_invoice(
            NigiriCln,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice,
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

        let invoice = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice,
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

        let invoice = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice,
            Some(THOUSAND_SATS),
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(NigiriLnd).unwrap().pub_key,
        );

        // Test open amount invoice (no amount specified)
        let invoice =
            nigiri::lnd_issue_invoice(LspdLnd, DESCRIPTION_SAMPLE, None, SECONDS_IN_AN_HOUR)
                .unwrap();

        let invoice = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice,
            None,
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(LspdLnd).unwrap().pub_key,
        );

        // Test invoice from different network (mainnet)
        let decode_result = node.decode_invoice(String::from(MAINNET_INVOICE));
        assert!(matches!(
            decode_result,
            Err(DecodeInvoiceError::NetworkMismatch { .. })
        ));
    }

    fn assert_invoice_details(
        invoice: Bolt11Invoice,
        amount_msat: Option<u64>,
        description: &str,
        expiry_time: Duration,
        payee_pub_key: &str,
    ) {
        assert_eq!(invoice.amount_milli_satoshis(), amount_msat);
        assert_eq!(
            invoice.description(),
            Bolt11InvoiceDescription::Direct(&Description::new(description.to_string()).unwrap())
        );
        assert_eq!(invoice.expiry_time(), expiry_time);
        let pub_key = match invoice.payee_pub_key() {
            None => invoice.recover_payee_pub_key().to_string(),
            Some(p) => p.to_string(),
        };
        assert_eq!(pub_key, payee_pub_key.to_string());
    }
}
