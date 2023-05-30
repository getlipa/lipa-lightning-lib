mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod persistence_test {
    use crate::setup::mocked_remote_storage::Config;
    use eel::errors::RuntimeErrorCode;
    use eel::interfaces::{ExchangeRateProvider, RemoteStorage};
    use eel::LightningNode;
    use log::info;
    use perro::Error::RuntimeError;
    use serial_test::file_serial;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{connect_node_to_lsp, mocked_storage_node_configurable, NodeHandle};
    use crate::setup_env::config::LOCAL_PERSISTENCE_PATH;
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};

    const ONE_SAT: u64 = 1_000;
    const TWO_K_SATS: u64 = 2_000_000;
    const HALF_M_SATS: u64 = 500_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn slow_remote_storage() {
        nigiri::setup_environment_with_lsp();
        let config = Config::new(Some(Duration::from_secs(1)), true, 100);
        let node_handle = mocked_storage_node_configurable(config);

        run_flow_normal_restart(&node_handle);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn unreliable_remote_storage() {
        nigiri::setup_environment_with_lsp();
        let config = Config::new(Some(Duration::from_secs(0)), true, 50);
        let node_handle = mocked_storage_node_configurable(config);

        run_flow_normal_restart(&node_handle);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn recovery() {
        nigiri::setup_environment_with_lsp();
        let config = Config::new(Some(Duration::from_secs(0)), true, 100);
        let node_handle = mocked_storage_node_configurable(config);

        run_flow_recovery_restart(&node_handle);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn unavailable_remote_storage() {
        nigiri::ensure_environment_running();

        let config = Config::new(None, false, 100);
        let node_handle = mocked_storage_node_configurable(config);

        let node_result = node_handle.start();
        assert!(matches!(
            node_result,
            Err(RuntimeError {
                code: RuntimeErrorCode::RemoteStorageError,
                ..
            })
        ));
    }

    fn run_flow_normal_restart<
        S: RemoteStorage + Clone + 'static,
        X: ExchangeRateProvider + Clone,
    >(
        node_handle: &NodeHandle<S, X>,
    ) {
        run_flow_1st_jit_channel(node_handle);

        // Wait for eel-node to shutdown
        sleep(Duration::from_secs(5));

        run_flow_2nd_jit_channel(node_handle);
    }

    fn run_flow_recovery_restart<
        S: RemoteStorage + Clone + 'static,
        X: ExchangeRateProvider + Clone,
    >(
        node_handle: &NodeHandle<S, X>,
    ) {
        run_flow_1st_jit_channel(node_handle);

        // Wait for eel-node to shutdown
        sleep(Duration::from_secs(5));
        // Recovery isn't possible while there's a local install
        assert!(matches!(
            node_handle.recover(),
            Err(perro::Error::InvalidInput { .. })
        ));
        // Remove the local state
        fs::remove_dir_all(LOCAL_PERSISTENCE_PATH).unwrap();
        // Perform recovery procedure
        node_handle.recover().unwrap();

        run_flow_2nd_jit_channel(node_handle);
    }

    fn run_flow_1st_jit_channel<
        S: RemoteStorage + Clone + 'static,
        X: ExchangeRateProvider + Clone,
    >(
        node_handle: &NodeHandle<S, X>,
    ) {
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
            );
            info!("Restarting node..."); // to test that channel monitors and manager are persisted and retrieved correctly
        } // Shut down the node
    }

    fn run_flow_2nd_jit_channel<
        S: RemoteStorage + Clone + 'static,
        X: ExchangeRateProvider + Clone,
    >(
        node_handle: &NodeHandle<S, X>,
    ) {
        {
            let node = node_handle.start_or_panic();

            // Wait for p2p connection to be reestablished and channels marked active
            wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

            run_jit_channel_open_flow(&node, NodeInstance::NigiriLnd, HALF_M_SATS, TWO_K_SATS);
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
            .create_invoice(payment_amount, "test".to_string(), String::new())
            .unwrap()
            .to_string();

        nigiri::pay_invoice(paying_node, &invoice).unwrap();

        assert_payment_received(node, initial_balance + payment_amount - lsp_fee);
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
