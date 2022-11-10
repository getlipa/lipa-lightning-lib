mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod chain_sync_test {
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::nigiri::{wait_for_sync, NodeInstance};
    use crate::setup::{nigiri, NodeHandle};
    use crate::try_cmd_repeatedly;
    use uniffi_lipalightninglib::config::NodeAddress;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    fn test_channel_is_confirmed_chain_only_after_6_confirmations() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &node_id, false).unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 5);

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
    }

    #[test]
    fn test_force_close_is_detected() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id =
            nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &node_id, false).unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 50);

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

        nigiri::lnd_node_disconnect_peer(NodeInstance::NigiriLnd, node_id).unwrap();
        nigiri::lnd_node_force_close_channel(NodeInstance::NigiriLnd, tx_id).unwrap();
        nigiri::lnd_node_stop(NodeInstance::NigiriLnd).unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 0);
    }

    #[test]
    fn test_channel_remains_usable_after_restart() {
        let node_handle = setup();

        start_node_open_confirm_channel_stop_node(&node_handle);

        let node = node_handle.start().unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
    }

    #[test]
    fn test_channel_is_confirmed_chain_only_after_6_confirmations_offline_node() {
        let node_handle = setup();

        start_node_open_channel_without_confirm_stop_node(&node_handle);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 6);
        // TODO: figure out why the following sleep is needed
        sleep(Duration::from_secs(5));

        let node = node_handle.start().unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
    }

    #[test]
    fn test_force_close_is_detected_offline_node() {
        let node_handle = setup();

        let tx_id = start_node_open_confirm_channel_stop_node(&node_handle);

        nigiri::lnd_node_force_close_channel(NodeInstance::NigiriLnd, tx_id).unwrap();
        // TODO: as soon as we regularly reconnect to peers, we can uncomment the following line
        //      as then we'll be able to handle not being connected to our peers
        // nigiri::lnd_stop().unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        let node = node_handle.start().unwrap();

        sleep(Duration::from_secs(10));

        // This only passes with the sleep that precedes it. TODO: confirm that's not a problem
        assert_eq!(node.get_node_info().channels_info.num_channels, 0);
    }

    #[test]
    fn test_force_close_is_detected_offline_node_unconfirmed_channel() {
        let node_handle = setup();

        let tx_id = start_node_open_channel_without_confirm_stop_node(&node_handle);

        nigiri::lnd_node_force_close_channel(NodeInstance::NigiriLnd, tx_id).unwrap();
        nigiri::lnd_node_stop(NodeInstance::NigiriLnd).unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        let node = node_handle.start().unwrap();

        sleep(Duration::from_secs(10));

        // This only passes with the sleep that precedes it. TODO: confirm that's not a problem
        assert_eq!(node.get_node_info().channels_info.num_channels, 0);
    }

    #[test]
    fn test_multiple_txs_simultaneously() {
        let node_handle = setup();
        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        // open 5 channels and force-close 3 of them right away
        let mut open_channels = open_5_chans_close_2(&node_id);

        // mine a block and do the same again and remove 1 of the previously opened channels
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);
        wait_for_sync(NodeInstance::NigiriLnd);
        let _ = open_5_chans_close_2(&node_id);
        nigiri::lnd_node_force_close_channel(NodeInstance::NigiriLnd, open_channels.pop().unwrap())
            .unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 3);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 3);
    }

    fn setup() -> NodeHandle {
        nigiri::start();
        let lsp_info = nigiri::query_lnd_node_info(NodeInstance::NigiriLnd).unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };

        let node_handle = NodeHandle::new(lsp_node);

        // to open multiple channels in the same block multiple UTXOs are required in LND
        for _ in 0..20 {
            nigiri::fund_lnd_node(NodeInstance::NigiriLnd, 0.5)
        }

        node_handle
    }

    fn start_node_open_confirm_channel_stop_node(node_handle: &NodeHandle) -> String {
        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id =
            nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &node_id, false).unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 6);

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

        tx_id
    }

    fn start_node_open_channel_without_confirm_stop_node(node_handle: &NodeHandle) -> String {
        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id =
            nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &node_id, false).unwrap();

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 0);

        tx_id
    }

    fn open_5_chans_close_2(node_id: &str) -> Vec<String> {
        let mut open_channels = Vec::new();

        for i in 0..5 {
            let tx_id =
                nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &node_id, false).unwrap();
            if i % 2 == 0 {
                nigiri::lnd_node_force_close_channel(NodeInstance::NigiriLnd, tx_id).unwrap();
            } else {
                open_channels.push(tx_id);
            }
        }

        open_channels
    }
}
