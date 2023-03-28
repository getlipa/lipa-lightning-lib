mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod sending_payments_test {
    use crate::setup::mocked_storage_node;
    use eel::{InvoiceDetails, LightningNode};
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance::{LspdLnd, NigiriCln, NigiriLnd};

    const REBALANCE_AMOUNT: u64 = 50_000_000;
    const CHANNEL_SIZE: u64 = 1_000_000_000;
    const PAYMENT_AMOUNT: u64 = 1_000_000;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn pay_invoice_direct_peer_test_and_invoice_decoding_test() {
        nigiri::setup_environment_with_lsp();
        let node = mocked_storage_node().start().unwrap();

        assert_eq!(node.get_node_info().num_peers, 1);
        nigiri::initiate_channel_from_remote(node.get_node_info().node_pubkey, LspdLnd);

        // Test hardcoded invoices here to avoid an additional test env set up
        invoice_decode_test(&node);

        assert!(node.get_node_info().channels_info.num_channels > 0);
        assert!(node.get_node_info().channels_info.num_usable_channels > 0);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > REBALANCE_AMOUNT);

        let invoice_details = node
            .create_invoice(REBALANCE_AMOUNT, "test".to_string(), String::new())
            .unwrap();
        assert!(invoice_details.invoice.starts_with("lnbc"));

        nigiri::pay_invoice(LspdLnd, &invoice_details.invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            REBALANCE_AMOUNT
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat < REBALANCE_AMOUNT); // because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat
                < CHANNEL_SIZE - REBALANCE_AMOUNT
        ); // smaller instead of equal because of channel reserves

        let invoice = nigiri::issue_invoice(LspdLnd, "test", PAYMENT_AMOUNT, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        node.pay_invoice(invoice, String::new()).unwrap();
        sleep(Duration::from_secs(2));

        let final_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        assert_eq!(final_balance - initial_balance, PAYMENT_AMOUNT);
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
            THOUSAND_SATS,
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
            THOUSAND_SATS,
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
            THOUSAND_SATS,
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(NigiriLnd).unwrap().pub_key,
        );
    }

    fn assert_invoice_details(
        invoice_details: InvoiceDetails,
        amount_msat: u64,
        description: &str,
        expiry_time: Duration,
        payee_pub_key: &str,
    ) {
        assert_eq!(invoice_details.amount_msat.unwrap(), amount_msat);
        assert_eq!(invoice_details.description, description);
        assert_eq!(invoice_details.expiry_interval, expiry_time);
        assert_eq!(invoice_details.payee_pub_key, payee_pub_key);
    }
}
