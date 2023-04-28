mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod rapid_gossip_sync_test {
    use crate::setup::mocked_storage_node;
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::{wait_for_new_channel_to_confirm, NodeInstance};
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};
    use bitcoin::hashes::hex::ToHex;
    use eel::LightningNode;
    use log::info;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    const HUNDRED_K_SATS: u64 = 100_000_000;
    const ONE_K_SATS: u64 = 1_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const LSPD_LND_HOST: &str = "lspd-lnd";
    const LSPD_LND_PORT: u16 = 9739;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_update_from_0_and_partial_update() {
        nigiri::setup_environment_with_lsp_rgs();
        let node_handle = mocked_storage_node();

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        let invoice_test_payment_retry =
            nigiri::issue_invoice(NodeInstance::NigiriCln, "test", ONE_K_SATS, 3600).unwrap();

        {
            let node = node_handle.start_or_panic();
            let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
            wait_for_eq!(node.get_node_info().num_peers, 1);

            // Setup channels:
            // NigiriLND -> LspdLnd  -> 3L
            // NigiriCLN -> LspdLnd
            nigiri::node_connect(
                NodeInstance::NigiriLnd,
                &lspd_node_id,
                LSPD_LND_HOST,
                LSPD_LND_PORT,
            )
            .unwrap();
            nigiri::node_connect(
                NodeInstance::NigiriCln,
                &lspd_node_id,
                LSPD_LND_HOST,
                LSPD_LND_PORT,
            )
            .unwrap();

            wait_for!(
                nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).is_ok()
            );
            wait_for!(
                nigiri::cln_node_open_pub_channel(NodeInstance::NigiriCln, &lspd_node_id).is_ok()
            );
            try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
            wait_for_new_channel_to_confirm(NodeInstance::LspdLnd, &lipa_node_id);
            wait_for_new_channel_to_confirm(NodeInstance::NigiriCln, &lspd_node_id);

            assert_eq!(node.get_node_info().channels_info.num_channels, 1);
            assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

            assert!(node.get_node_info().channels_info.inbound_capacity_sat > 2 * HUNDRED_K_SATS);

            // Pay from NigiriCln to 3L to create outbound liquidity
            let invoice_cln = node
                .create_invoice(HUNDRED_K_SATS, "test".to_string(), String::new())
                .unwrap();
            assert!(invoice_cln.invoice.starts_with("lnbc"));

            nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice_cln.invoice).unwrap();

            assert_eq!(
                node.get_node_info().channels_info.local_balance_sat,
                HUNDRED_K_SATS
            );
            wait_for!(node.get_node_info().channels_info.outbound_capacity_sat > 0);

            // The node hasn't yet learned about the new channels so it won't be able to pay
            assert!(matches!(
                node.pay_invoice(invoice_test_payment_retry.clone(), String::new()),
                Err(perro::Error::RuntimeError {
                    code: eel::errors::RuntimeErrorCode::NoRouteFound,
                    ..
                })
            ));

            // wait for the RGS server to learn about the new channels (100 seconds isn't enough)
            sleep(Duration::from_secs(150));

            node.foreground();

            info!("Restarting node..."); // to test that the graph is persisted and retrieved correctly
        } // Shut down the node

        // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        {
            let node = node_handle.start_or_panic();

            // Wait for p2p connection to be reestablished and channels marked active
            sleep(Duration::from_secs(5));
            assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

            send_payment_flow(&node, NodeInstance::NigiriCln, ONE_K_SATS);

            // If paying an invoice has failed, retrying is possible
            node.pay_invoice(invoice_test_payment_retry, String::new())
                .unwrap();

            // Create new channel - the 3L node will have to learn about it in a partial sync
            nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false)
                .unwrap();
            try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
            wait_for_new_channel_to_confirm(NodeInstance::NigiriLnd, &lspd_node_id);

            // Pay from NigiriLnd to 3L to create outbound liquidity (LspdLnd -> NigiriLnd)
            let invoice_lnd = node
                .create_invoice(HUNDRED_K_SATS, "test".to_string(), String::new())
                .unwrap();
            assert!(invoice_lnd.invoice.starts_with("lnbc"));

            nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice_lnd.invoice).unwrap();

            // wait for the RGS server to learn about the new channels (100 seconds isn't enough)
            sleep(Duration::from_secs(150));

            node.foreground();

            info!("Restarting node..."); // to test that the graph is persisted and retrieved correctly
        } // Shut down the node

        // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        {
            let node = node_handle.start_or_panic();

            // Wait for p2p connection to be reestablished and channels marked active
            sleep(Duration::from_secs(5));
            assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

            send_payment_flow(&node, NodeInstance::NigiriLnd, ONE_K_SATS);
        }
    }

    fn send_payment_flow(node: &LightningNode, target: NodeInstance, amount_msat: u64) {
        let invoice = nigiri::issue_invoice(target, "test", amount_msat, 3600).unwrap();
        let initial_balance = nigiri::query_node_balance(target).unwrap();
        node.pay_invoice(invoice, String::new()).unwrap();
        sleep(Duration::from_secs(2));
        let final_balance = nigiri::query_node_balance(target).unwrap();
        assert_eq!(final_balance - initial_balance, amount_msat);
    }
}
