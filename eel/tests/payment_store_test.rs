mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod receiving_payments_test {
    use bitcoin::Network;
    use eel::payment::{Payment, PaymentState, PaymentType, TzTime};
    use eel::InvoiceDetails;
    use log::info;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};

    use crate::setup::{
        connect_node_to_lsp, issue_invoice, mocked_storage_node, setup_outbound_capacity,
    };
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::setup_env::nigiri::NodeInstance::{LspdLnd, NigiriLnd};
    use crate::{eq_or_err, try_cmd_repeatedly, wait_for, wait_for_eq, wait_for_ok};

    const ONE_SAT: u64 = 1_000;
    const TWO_K_SATS: u64 = 2_000_000;
    const TWENTY_K_SATS: u64 = 20_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_payment_store_by_amount_of_entries() {
        nigiri::setup_environment_with_lsp();
        let node_handle = mocked_storage_node();

        {
            let node = node_handle.start_or_panic();
            wait_for_eq!(node.get_node_info().num_peers, 1);

            let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
                .unwrap()
                .pub_key;

            connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);

            nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false)
                .unwrap();
            try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
            wait_for!(nigiri::is_channel_confirmed(
                NodeInstance::NigiriLnd,
                &lspd_node_id
            ));

            let invoice = issue_invoice(&node, TWO_K_SATS + ONE_SAT); // LSP minimum + 1
            nigiri::pay_invoice(NodeInstance::NigiriLnd, &invoice).unwrap();

            assert!(matches!(
                node.get_latest_payments(0),
                Err(perro::Error::InvalidInput { .. })
            ));
            assert_eq!(node.get_latest_payments(1).unwrap().len(), 1);
            assert_eq!(node.get_latest_payments(2).unwrap().len(), 1);

            info!("Restarting node...");
        } // Shut down the node

        // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        // After restarting the node, the payment should still be in the store
        let node = node_handle.start_or_panic();
        assert_eq!(node.get_latest_payments(2).unwrap().len(), 1);

        // Wait for p2p connection to be reestablished and channels marked active
        wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

        // Receive another payment
        let invoice = issue_invoice(&node, TWENTY_K_SATS);
        nigiri::pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();

        assert_eq!(node.get_latest_payments(1).unwrap().len(), 1);
        assert_eq!(node.get_latest_payments(2).unwrap().len(), 2);
        assert_eq!(node.get_latest_payments(u32::MAX).unwrap().len(), 2);

        // Test sending payment
        let invoice = nigiri::issue_invoice(LspdLnd, "test", ONE_SAT, 3600).unwrap();
        node.pay_invoice(invoice, String::new()).unwrap();

        assert_eq!(node.get_latest_payments(15).unwrap().len(), 3);

        // test get_payment function
        for payment in node.get_latest_payments(3).unwrap() {
            assert_eq!(&payment, &node.get_payment(&payment.hash).unwrap());
        }
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_payment_store_for_received_payments() {
        nigiri::setup_environment_with_lsp();
        let node_handle = mocked_storage_node();
        let node = node_handle.start_or_panic();

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;
        connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);
        nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        wait_for_eq!(nigiri::get_number_of_txs_in_mempool(), Ok::<u64, String>(1));
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);

        let invoice_details = node
            .create_invoice(TWENTY_K_SATS, "test".to_string(), String::new())
            .unwrap();
        assert!(invoice_details.invoice.starts_with("lnbc"));

        let dummy_timestamp = TzTime {
            time: SystemTime::now(),
            timezone_id: "Could be anything.".to_string(),
            timezone_utc_offset_secs: 0,
        };

        let payment_dummy = Payment {
            payment_type: PaymentType::Receiving,
            payment_state: PaymentState::Succeeded,
            hash: "<unknown>".to_string(),
            amount_msat: TWENTY_K_SATS,
            invoice_details: invoice_details.clone(),
            created_at: dummy_timestamp.clone(),
            latest_state_change_at: dummy_timestamp,
            description: "test".to_string(),
            preimage: None,
            network_fees_msat: None,
            lsp_fees_msat: Some(node.calculate_lsp_fee(TWENTY_K_SATS).unwrap()),
            fiat_values: node.get_fiat_values(TWENTY_K_SATS), // Todo: What should be persisted? The fiat value of what the payer sended, or the fiat value of what the lipa user received (subtracting potential LSP fee)?
            metadata: "".to_string(),
        };

        wait_for_ok!(nigiri::pay_invoice(NigiriLnd, &invoice_details.invoice));
        assert_payments_are_partially_equal(
            node.get_latest_payments(10).unwrap().first().unwrap(),
            &payment_dummy,
        )
        .unwrap();
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_payment_store_for_sent_payments() {
        nigiri::setup_environment_with_lsp();
        let node_handle = mocked_storage_node();
        let node = node_handle.start_or_panic();

        setup_outbound_capacity(&node);
        assert_eq!(node.get_latest_payments(10).unwrap().len(), 1);

        let invoice = nigiri::issue_invoice(LspdLnd, "Fixed amount", TWO_K_SATS, 3600).unwrap();

        let dummy_timestamp = TzTime {
            time: SystemTime::now(),
            timezone_id: "Could be anything.".to_string(),
            timezone_utc_offset_secs: 0,
        };

        let mut payment_dummy = Payment {
            payment_type: PaymentType::Sending,
            payment_state: PaymentState::Created,
            hash: "<unknown>".to_string(),
            amount_msat: TWO_K_SATS,
            invoice_details: InvoiceDetails {
                invoice: invoice.clone(),
                amount_msat: Some(TWO_K_SATS),
                description: "Fixed amount".to_string(),
                payment_hash: "<unknown>".to_string(),
                payee_pub_key: nigiri::query_node_info(LspdLnd).unwrap().pub_key,
                creation_timestamp: SystemTime::now(),
                expiry_interval: Duration::from_secs(3600),
                expiry_timestamp: SystemTime::now(),
                network: Network::Regtest,
            },
            created_at: dummy_timestamp.clone(),
            latest_state_change_at: dummy_timestamp,
            description: "Fixed amount".to_string(),
            preimage: None,
            network_fees_msat: None,
            lsp_fees_msat: None,
            fiat_values: node.get_fiat_values(TWO_K_SATS),
            metadata: "".to_string(),
        };

        // Fixed amount invoice
        {
            node.pay_invoice(invoice, String::new()).unwrap();
            assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            )
            .unwrap();

            payment_dummy.payment_state = PaymentState::Succeeded;
            payment_dummy.network_fees_msat = Some(0);
            wait_for_ok!(assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            ));
        }

        // Open amount invoice
        {
            let invoice = nigiri::lnd_issue_invoice(LspdLnd, "Open amount", None, 3600).unwrap();

            payment_dummy.payment_state = PaymentState::Created;
            payment_dummy.amount_msat = ONE_SAT;
            payment_dummy.description = "Open amount".to_string();
            payment_dummy.network_fees_msat = None;
            payment_dummy.fiat_values = node.get_fiat_values(ONE_SAT);
            payment_dummy.metadata = "Some metadata".to_string();
            payment_dummy.invoice_details.invoice = invoice.clone();
            payment_dummy.invoice_details.description = "Open amount".to_string();
            payment_dummy.invoice_details.amount_msat = None;

            node.pay_open_invoice(invoice, ONE_SAT, "Some metadata".to_string())
                .unwrap();

            assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            )
            .unwrap();

            payment_dummy.payment_state = PaymentState::Succeeded;
            payment_dummy.network_fees_msat = Some(0);
            wait_for_ok!(assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            ));
        }
    }

    fn assert_payments_are_partially_equal(left: &Payment, right: &Payment) -> Result<(), String> {
        eq_or_err!(left.payment_type, right.payment_type);
        eq_or_err!(left.payment_state, right.payment_state);
        eq_or_err!(left.amount_msat, right.amount_msat);
        eq_or_err!(left.invoice_details.invoice, right.invoice_details.invoice);
        eq_or_err!(
            left.invoice_details.amount_msat,
            right.invoice_details.amount_msat
        );
        eq_or_err!(
            left.invoice_details.description,
            right.invoice_details.description
        );
        eq_or_err!(left.invoice_details.network, right.invoice_details.network);
        eq_or_err!(left.description, right.description);
        eq_or_err!(left.network_fees_msat, right.network_fees_msat);
        eq_or_err!(left.lsp_fees_msat, right.lsp_fees_msat);
        eq_or_err!(left.fiat_values, right.fiat_values);
        eq_or_err!(left.metadata, right.metadata);

        Ok(())
    }
}
