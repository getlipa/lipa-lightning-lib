mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod chain_sync_test {
    use super::*;
    use bitcoin::hashes::hex::ToHex;
    use std::time::Duration;

    use crate::setup::{nigiri, NodeHandle};
    use uniffi_lipalightninglib::config::NodeAddress;

    const HALF_SEC: Duration = Duration::from_millis(500);

    #[test]
    fn test_zero_conf_channel_is_usable_without_confirmations() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        let address = nigiri::get_lspd_lnd_address().unwrap();
        nigiri::fund_lspd_lnd_node(0.5, address).unwrap();

        nigiri::lspd_lnd_open_zero_conf_channel(&node_id).unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }

    fn setup() -> NodeHandle {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lspd_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9739".to_string(),
        };

        let node_handle = NodeHandle::new(lsp_node);

        nigiri::try_cmd_repeatedly(nigiri::fund_lnd_node, 0.5, 10, HALF_SEC).unwrap();

        node_handle
    }
}
