mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod receiving_payments_test {
    use bitcoin::hashes::hex::ToHex;
    use eel::LightningNode;
    use log::info;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{connect_node_to_lsp, issue_invoice, mocked_storage_node};
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};

    const ONE_SAT: u64 = 1_000;
    const ONE_K_SATS: u64 = 1_000_000;
    const TWO_K_SATS: u64 = 2_000_000;
    const TEN_K_SATS: u64 = 10_000_000;
    const TWENTY_K_SATS: u64 = 20_000_000;
    const HALF_M_SATS: u64 = 500_000_000;

    // The amount of sats the LSP provides to the user as inbound capacity.
    // See https://github.com/getlipa/lipa-lightning-lib/blob/b821162df982799c497e083e9707aa421aee43a8/lspd/compose.yaml#LL63C44-L63C46
    const LSP_TOPUP: u64 = 100_000_000; // 100K sats

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_multiple_receive_scenarios() {
        // Test receiving an invoice on a node that does not have any channel yet
        // resp, the channel opening is part of the payment process.
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

            run_jit_channel_open_flow(
                &node,
                NodeInstance::NigiriLnd,
                TWO_K_SATS + ONE_SAT,
                TWO_K_SATS,
                LSP_TOPUP,
            );
            info!("Restarting node..."); // to test that channel monitors and manager are persisted and retrieved correctly
        } // Shut down the node

        // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        {
            let node = node_handle.start_or_panic();

            // Wait for p2p connection to be reestablished and channels marked active
            wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

            // Test receiving an amount that needs a new channel open when we already have existing channels.
            // We should have 102001 sat channel and have received a 1 sat payment. A 0.5M payment is not
            // possible. A new channel with 0.6M size should be created
            run_jit_channel_open_flow(
                &node,
                NodeInstance::NigiriLnd,
                HALF_M_SATS,
                TWO_K_SATS,
                LSP_TOPUP,
            );
            assert_eq!(node.get_node_info().channels_info.num_usable_channels, 2);
            info!("Restarting node..."); // to test that the graph is persisted and retrieved correctly
        } // Shut down the node

        // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        {
            let node = node_handle.start_or_panic();

            // Wait for p2p connection to be reestablished and channels marked active
            wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 2);

            // Test receiving an invoice on a node that already has an open channel
            run_payment_flow(&node, NodeInstance::LspdLnd, TWENTY_K_SATS);

            // The difference between sending 1_000 sats and 20_000 sats is that receiving 1_000 sats
            // creates a dust-HTLC, while receiving 20_000 sats does not.
            // A dust-HTLC is an HTLC that is too small to be worth the fees to settle it.
            run_payment_flow(&node, NodeInstance::LspdLnd, ONE_K_SATS);

            // Previously receiving 10K sats failed because it results in a dust htlc which was above
            // the default max dust htlc exposure.
            run_payment_flow(&node, NodeInstance::LspdLnd, TEN_K_SATS);

            // Receive multiple payments
            let initial_balance = node.get_node_info().channels_info.local_balance_msat;
            let amt_of_payments = 10;
            assert_channel_ready(&node, TWO_K_SATS * amt_of_payments);
            for i in 1..=amt_of_payments {
                let invoice = issue_invoice(&node, TWO_K_SATS);

                nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();
                assert_eq!(
                    node.get_node_info().channels_info.local_balance_msat,
                    initial_balance + TWO_K_SATS * i
                );
            }
            assert_payment_received(&node, initial_balance + TWO_K_SATS * amt_of_payments);
        }
    }

    // This also tests that payments with a hop work and as such, routing hints are being correctly
    // included in the created invoices
    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn receive_multiple_payments_for_same_invoice() {
        nigiri::ensure_environment_running();

        let node = mocked_storage_node().start_or_panic();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        wait_for_eq!(node.get_node_info().num_peers, 1);

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);
        connect_node_to_lsp(NodeInstance::NigiriCln, &lspd_node_id);
        wait_for!(nigiri::cln_owns_utxos(NodeInstance::NigiriCln).unwrap());

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        nigiri::cln_node_open_pub_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        wait_for!(nigiri::is_channel_confirmed(
            NodeInstance::LspdLnd,
            &lipa_node_id
        ));
        wait_for!(nigiri::is_channel_confirmed(
            NodeInstance::NigiriLnd,
            &lspd_node_id
        ));
        wait_for!(nigiri::is_channel_confirmed(
            NodeInstance::NigiriCln,
            &lspd_node_id
        ));

        assert_channel_ready(&node, TWENTY_K_SATS * 3);
        let invoice = issue_invoice(&node, TWENTY_K_SATS);

        nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice).unwrap();
        // New attempts to pay the same invoice should fail!
        assert!(nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).is_err());
        assert!(nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice).is_err());

        assert_payment_received(&node, TWENTY_K_SATS);
    }

    fn run_jit_channel_open_flow(
        node: &LightningNode,
        paying_node: NodeInstance,
        payment_amount: u64,
        lsp_fee: u64,
        lsp_topup: u64,
    ) {
        let channels_info = node.get_node_info().channels_info;
        let initial_balance = channels_info.local_balance_msat;
        let initial_channel_capacity = channels_info.total_channel_capacities_msat;

        let invoice = issue_invoice(&node, payment_amount);

        nigiri::pay_invoice(paying_node, &invoice).unwrap();

        assert_channel_capacity(&node, initial_channel_capacity + payment_amount + lsp_topup);
        assert_payment_received(&node, initial_balance + payment_amount - lsp_fee);
    }

    fn run_payment_flow(node: &LightningNode, paying_node: NodeInstance, payment_amount: u64) {
        let initial_balance = node.get_node_info().channels_info.local_balance_msat;

        assert_channel_ready(&node, payment_amount);
        let invoice = issue_invoice(&node, payment_amount);

        nigiri::pay_invoice(paying_node, &invoice).unwrap();

        assert_payment_received(&node, initial_balance + payment_amount);
    }

    fn assert_channel_ready(node: &LightningNode, payment_amount: u64) {
        assert!(node.get_node_info().channels_info.num_channels > 0);
        assert!(node.get_node_info().channels_info.num_usable_channels > 0);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > payment_amount);
    }

    fn assert_channel_capacity(node: &LightningNode, expected_capacity: u64) {
        let new_capacity = node
            .get_node_info()
            .channels_info
            .total_channel_capacities_msat;

        assert_eq!(new_capacity, expected_capacity);
    }

    fn assert_payment_received(node: &LightningNode, expected_balance: u64) {
        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            expected_balance
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat < expected_balance);
        // because of channel reserves
    }
}
