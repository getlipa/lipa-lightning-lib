mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod chain_sync_test {
    use bitcoin::hashes::hex::ToHex;
    use eel::interfaces::RemoteStorage;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{mocked_storage_node, NodeHandle};
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::{wait_for_new_channel_to_confirm, NodeInstance};
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_react_to_events() {
        nigiri::setup_environment_with_lsp();

        let node = mocked_storage_node().start_or_panic();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id = nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 5);

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        wait_for_new_channel_to_confirm(NodeInstance::LspdLnd, &node_id);

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

        // test multiple txs simultaneously
        let node_id = node.get_node_info().node_pubkey.to_hex();
        let mut open_channels = open_2_chans_close_1(&node_id);

        // mine a block and do the same again and remove 1 of the previously opened channels
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);
        wait_for_eq!(node.get_node_info().channels_info.num_channels, 2);

        wait_for!(nigiri::is_node_synced(NodeInstance::LspdLnd));
        let _ = open_2_chans_close_1(&node_id);
        nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, open_channels.pop().unwrap())
            .unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);

        wait_for_eq!(node.get_node_info().channels_info.num_channels, 2);
        wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 2);

        // test force close is detected
        nigiri::lnd_node_disconnect_peer(NodeInstance::LspdLnd, node_id).unwrap();
        nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, tx_id).unwrap();
        nigiri::node_stop(NodeInstance::LspdLnd).unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 2);
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);
        wait_for_eq!(node.get_node_info().channels_info.num_channels, 1);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_react_to_events_with_offline_node() {
        nigiri::setup_environment_with_lsp();

        let node_handle = mocked_storage_node();

        // test channel is confirmed only after 6 confirmations with offline node
        let tx_id = start_node_open_channel_without_confirm_stop_node(&node_handle);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 6);
        // TODO: figure out why the following sleep is needed
        sleep(Duration::from_secs(5));

        {
            let node = node_handle.start_or_panic();

            assert_eq!(node.get_node_info().channels_info.num_channels, 1);
            wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        } // drop node

        // test node remains usable after restart
        {
            let node = node_handle.start_or_panic();

            assert_eq!(node.get_node_info().channels_info.num_channels, 1);
            wait_for_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        }

        // test force close is detected offline node
        nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, tx_id).unwrap();
        nigiri::node_stop(NodeInstance::LspdLnd).unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        let node = node_handle.start_or_panic();

        // Wait for the local node to learn from esplora that the channel has been force closed
        wait_for_eq!(node.get_node_info().channels_info.num_channels, 0);
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_force_close_is_detected_offline_node_unconfirmed_channel() {
        nigiri::setup_environment_with_lsp();
        let node_handle = mocked_storage_node();

        let tx_id = start_node_open_channel_without_confirm_stop_node(&node_handle);
        log::debug!("Eel node stopped");

        nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, tx_id).unwrap();
        nigiri::node_stop(NodeInstance::LspdLnd).unwrap();
        log::debug!("Nigiri node stopped");

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        let node = node_handle.start_or_panic();

        // Wait for the local node to learn that the channel has been force closed (esplora sync)
        wait_for_eq!(node.get_node_info().channels_info.num_channels, 0);
    }

    fn start_node_open_channel_without_confirm_stop_node<S: RemoteStorage + Clone + 'static>(
        node_handle: &NodeHandle<S>,
    ) -> String {
        let node = node_handle.start_or_panic();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id = nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        tx_id
    }

    // open 2 channels and force-close 1 of them right away
    fn open_2_chans_close_1(node_id: &str) -> Vec<String> {
        let mut open_channels = Vec::new();

        for i in 0..2 {
            let tx_id =
                nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
            if i % 2 == 0 {
                nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, tx_id).unwrap();
            } else {
                open_channels.push(tx_id);
            }
        }

        open_channels
    }
}
