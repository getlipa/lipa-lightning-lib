mod setup;

#[cfg(feature = "nigiri")]
mod persistence_test {
    use crate::setup::mocked_remote_storate::Config;
    use eel::LightningNode;
    use log::info;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::nigiri::NodeInstance;
    use crate::setup::{nigiri, NodeHandle};
    use crate::try_cmd_repeatedly;

    const ONE_SAT: u64 = 1_000;
    const TWO_K_SATS: u64 = 2_000_000;
    const FIVE_HUNDRED_K_SATS: u64 = 500_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const LSPD_LND_HOST: &str = "lspd-lnd";
    const LSPD_LND_PORT: u16 = 9739;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn slow_remote_storage() {
        nigiri::setup_environment_with_lsp();
        let node_handle = NodeHandle::new(Config::new(Some(Duration::from_secs(1)), true, 100));

        run_flow(&node_handle);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn unreliable_remote_storage() {
        nigiri::setup_environment_with_lsp();
        let node_handle = NodeHandle::new(Config::new(Some(Duration::from_secs(0)), true, 50));

        run_flow(&node_handle);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn unavailable_remote_storage() {
        nigiri::ensure_environment_running();

        let node_handle = NodeHandle::new(Config::new(None, false, 100));

        let node_result = node_handle.start();
        assert!(node_result.is_err());
        assert!(node_result
            .err()
            .unwrap()
            .to_string()
            .contains("RemoteStorageServiceUnavailable"));
    }

    fn run_flow(node_handle: &NodeHandle) {
        {
            let node = node_handle.start().unwrap();
            assert_eq!(node.get_node_info().num_peers, 1);

            let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
                .unwrap()
                .pub_key;

            connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);

            nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false)
                .unwrap();
            try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
            nigiri::wait_for_new_channel_to_confirm(NodeInstance::NigiriLnd, &lspd_node_id);

            run_jit_channel_open_flow(
                &node,
                NodeInstance::NigiriLnd,
                TWO_K_SATS + ONE_SAT,
                TWO_K_SATS,
            );
            info!("Restarting node..."); // to test that channel monitors and manager are persisted and retrieved correctly
        } // Shut down the node

        // Wait for shutdown to complete
        sleep(Duration::from_secs(5));

        {
            let node = node_handle.start().unwrap();

            // Wait for p2p connection to be reestablished and channels marked active
            sleep(Duration::from_secs(5));
            assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

            run_jit_channel_open_flow(
                &node,
                NodeInstance::NigiriLnd,
                FIVE_HUNDRED_K_SATS,
                TWO_K_SATS,
            );
            assert_eq!(node.get_node_info().channels_info.num_usable_channels, 2);
        }
    }

    fn run_jit_channel_open_flow(
        node: &LightningNode,
        paying_node: NodeInstance,
        payment_amount: u64,
        lsp_fee: u64,
    ) {
        let initial_balance = node.get_node_info().channels_info.local_balance_msat;

        let invoice = node
            .create_invoice(payment_amount, "test".to_string())
            .unwrap();

        nigiri::pay_invoice(paying_node, &invoice).unwrap();

        assert_payment_received(&node, initial_balance + payment_amount - lsp_fee);
    }

    fn assert_payment_received(node: &LightningNode, expected_balance: u64) {
        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            expected_balance
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat < expected_balance);
        // because of channel reserves
    }

    fn connect_node_to_lsp(node: NodeInstance, lsp_node_id: &str) {
        nigiri::node_connect(node, lsp_node_id, LSPD_LND_HOST, LSPD_LND_PORT).unwrap();
    }
}
