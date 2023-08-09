mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod data_store_test {
    use eel::payment::{Payment, PaymentState, PaymentType, TzTime};
    use eel::Bolt11Invoice;
    use log::info;
    use serial_test::file_serial;
    use std::str::FromStr;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime};

    use crate::setup::{
        connect_node_to_lsp, issue_invoice, mocked_storage_node, setup_outbound_capacity,
    };
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::setup_env::nigiri::NodeInstance::{LspdLnd, NigiriLnd};
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq, wait_for_ok};

    const ONE_SAT: u64 = 1_000;
    const TWO_K_SATS: u64 = 2_000_000;
    const TWENTY_K_SATS: u64 = 20_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_payment_storage_by_amount_of_entries() {
        nigiri::setup_environment_with_lsp();
        let node_handle = mocked_storage_node();

        {
            let node = node_handle.start_or_panic();
            wait_for!(!node.get_node_info().peers.is_empty());

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
    fn test_payment_storage_for_received_payments() {
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

        let invoice = node
            .create_invoice(TWENTY_K_SATS, "test".to_string(), String::new())
            .unwrap();
        assert!(invoice.to_string().starts_with("lnbc"));

        let dummy_timestamp = TzTime {
            time: SystemTime::now(),
            timezone_id: "Could be anything.".to_string(),
            timezone_utc_offset_secs: 0,
        };

        let payment_dummy = Payment {
            payment_type: PaymentType::Receiving,
            payment_state: PaymentState::Succeeded,
            fail_reason: None,
            hash: "<unknown>".to_string(),
            amount_msat: TWENTY_K_SATS,
            invoice: invoice.clone(),
            created_at: dummy_timestamp.clone(),
            latest_state_change_at: dummy_timestamp,
            description: "test".to_string(),
            preimage: None,
            network_fees_msat: None,
            lsp_fees_msat: Some(node.calculate_lsp_fee(TWENTY_K_SATS).unwrap()),
            exchange_rate: node.get_exchange_rate(),
            metadata: "".to_string(),
        };

        wait_for_ok!(nigiri::pay_invoice(NigiriLnd, &invoice.to_string()));
        assert_payments_are_partially_equal(
            node.get_latest_payments(10).unwrap().first().unwrap(),
            &payment_dummy,
        );
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_payment_storage_for_sent_payments() {
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
            fail_reason: None,
            hash: "<unknown>".to_string(),
            amount_msat: TWO_K_SATS,
            invoice: Bolt11Invoice::from_str(&invoice).unwrap(),
            created_at: dummy_timestamp.clone(),
            latest_state_change_at: dummy_timestamp,
            description: "Fixed amount".to_string(),
            preimage: None,
            network_fees_msat: None,
            lsp_fees_msat: None,
            exchange_rate: node.get_exchange_rate(),
            metadata: "".to_string(),
        };

        // Fixed amount invoice
        {
            node.pay_invoice(invoice, String::new()).unwrap();
            assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            );

            payment_dummy.payment_state = PaymentState::Succeeded;
            payment_dummy.network_fees_msat = Some(0);
            wait_for_eq!(
                node.get_latest_payments(10)
                    .unwrap()
                    .first()
                    .unwrap()
                    .payment_state,
                payment_dummy.payment_state
            );
            assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            );
        }

        // Open amount invoice
        {
            let invoice = nigiri::lnd_issue_invoice(LspdLnd, "Open amount", None, 3600).unwrap();

            payment_dummy.payment_state = PaymentState::Created;
            payment_dummy.amount_msat = ONE_SAT;
            payment_dummy.description = "Open amount".to_string();
            payment_dummy.network_fees_msat = None;
            payment_dummy.exchange_rate = node.get_exchange_rate();
            payment_dummy.metadata = "Some metadata".to_string();
            payment_dummy.invoice = Bolt11Invoice::from_str(&invoice).unwrap();

            node.pay_open_invoice(invoice, ONE_SAT, "Some metadata".to_string())
                .unwrap();

            assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            );

            payment_dummy.payment_state = PaymentState::Succeeded;
            payment_dummy.network_fees_msat = Some(0);
            wait_for_eq!(
                node.get_latest_payments(10)
                    .unwrap()
                    .first()
                    .unwrap()
                    .payment_state,
                payment_dummy.payment_state
            );
            assert_payments_are_partially_equal(
                node.get_latest_payments(10).unwrap().first().unwrap(),
                &payment_dummy,
            );
        }
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_exchange_rate_storage() {
        //nigiri::setup_environment_with_lsp();
        let mut node_handle = mocked_storage_node();

        node_handle.get_exchange_rate_provider().disable();

        {
            let node = node_handle.start_or_panic();

            sleep(Duration::from_secs(1));
            assert!(node.get_exchange_rate().is_none());

            info!("Restarting node...");
        } // Shut down the node
          // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        node_handle.get_exchange_rate_provider().enable();

        {
            let node = node_handle.start_or_panic();

            sleep(Duration::from_secs(1));
            assert!(node.get_exchange_rate().is_some());

            info!("Restarting node...");
        } // Shut down the node
          // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        node_handle.get_exchange_rate_provider().disable();

        let node = node_handle.start_or_panic();
        sleep(Duration::from_secs(1));
        assert!(node.get_exchange_rate().is_some());
    }

    fn assert_payments_are_partially_equal(left: &Payment, right: &Payment) {
        assert_eq!(left.payment_type, right.payment_type);
        assert_eq!(left.payment_state, right.payment_state);
        assert_eq!(left.amount_msat, right.amount_msat);
        assert_eq!(left.invoice.to_string(), right.invoice.to_string());
        assert_eq!(left.description, right.description);
        assert_eq!(left.network_fees_msat, right.network_fees_msat);
        assert_eq!(left.lsp_fees_msat, right.lsp_fees_msat);
        assert_eq!(left.exchange_rate, right.exchange_rate);
        assert_eq!(left.metadata, right.metadata);
    }
}
