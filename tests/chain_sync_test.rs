mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod chain_sync_test {
    use super::*;
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{nigiri, NodeHandle};
    use uniffi_lipalightninglib::config::NodeAddress;

    const HALF_SEC: Duration = Duration::from_millis(500);

    #[test]
    fn test_channel_is_confirmed_chain_only_after_6_confirmations() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        nigiri::lnd_open_channel(&node_id).unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 0);

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 5, 10, HALF_SEC).unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 0);

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 1, 10, HALF_SEC).unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }

    #[test]
    fn test_force_close_is_detected() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id = nigiri::lnd_open_channel(&node_id).unwrap();

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 50, 10, HALF_SEC).unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);

        nigiri::lnd_disconnect_peer(node_id).unwrap();
        nigiri::lnd_force_close_channel(tx_id).unwrap();
        nigiri::lnd_stop().unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 1, 10, HALF_SEC).unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 0);
    }

    #[test]
    fn test_channel_remains_usable_after_restart() {
        let node_handle = setup();

        start_node_open_confirm_channel_stop_node(&node_handle);

        let node = node_handle.start().unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }

    #[test]
    fn test_channel_is_confirmed_chain_only_after_6_confirmations_offline_node() {
        let node_handle = setup();

        start_node_open_channel_without_confirm_stop_node(&node_handle);

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 6, 10, HALF_SEC).unwrap();
        // TODO: figure out why the following sleep is needed
        sleep(Duration::from_secs(5));

        let node = node_handle.start().unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }

    #[test]
    fn test_force_close_is_detected_offline_node() {
        let node_handle = setup();

        let tx_id = start_node_open_confirm_channel_stop_node(&node_handle);

        nigiri::lnd_force_close_channel(tx_id).unwrap();
        // TODO: as soon as we regularly reconnect to peers, we can uncomment the following line
        //      as then we'll be able to handle not being connected to our peers
        // nigiri::lnd_stop().unwrap();

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 1, 10, HALF_SEC).unwrap();

        let node = node_handle.start().unwrap();

        sleep(Duration::from_secs(10));

        // This only passes with the sleep that precedes it. TODO: confirm that's not a problem
        assert_eq!(node.get_node_info().num_channels, 0);
    }

    #[test]
    fn test_force_close_is_detected_offline_node_unconfirmed_channel() {
        let node_handle = setup();

        let tx_id = start_node_open_channel_without_confirm_stop_node(&node_handle);

        nigiri::lnd_force_close_channel(tx_id).unwrap();
        // TODO: as soon as we regularly reconnect to peers, we can uncomment the following line
        //      as then we'll be able to handle not being connected to our peers
        // nigiri::lnd_stop().unwrap();

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 1, 10, HALF_SEC).unwrap();

        let node = node_handle.start().unwrap();

        sleep(Duration::from_secs(10));

        // This only passes with the sleep that precedes it. TODO: confirm that's not a problem
        assert_eq!(node.get_node_info().num_channels, 0);
    }

    fn setup() -> NodeHandle {
        nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };

        let node_handle = NodeHandle::new(lsp_node);

        nigiri::try_cmd_repeatedly(nigiri::fund_lnd_node, 0.5, 10, HALF_SEC).unwrap();

        node_handle
    }

    fn start_node_open_confirm_channel_stop_node(node_handle: &NodeHandle) -> String {
        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id = nigiri::lnd_open_channel(&node_id).unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 0);

        nigiri::try_cmd_repeatedly(nigiri::mine_blocks, 6, 10, HALF_SEC).unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);

        tx_id
    }

    fn start_node_open_channel_without_confirm_stop_node(node_handle: &NodeHandle) -> String {
        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let tx_id = nigiri::lnd_open_channel(&node_id).unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 0);

        tx_id
    }
}
