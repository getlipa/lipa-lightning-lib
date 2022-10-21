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

    use crate::setup::nigiri;
    use uniffi_lipalightninglib::config::NodeAddress;

    #[test]
    fn test_channel_is_confirmed_chain_after_6_confirmations() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };

        let node = setup::setup(lsp_node).unwrap();
        assert_eq!(node.get_node_info().num_peers, 1);

        let node_id = node.get_node_info().node_pubkey.to_hex();

        nigiri::try_function_multiple_times(
            nigiri::fund_lnd_node,
            0.5,
            10,
            Duration::from_millis(200),
        )
        .unwrap();

        nigiri::lnd_open_channel(&node_id).unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 0);

        nigiri::try_function_multiple_times(nigiri::mine_blocks, 6, 10, Duration::from_millis(200))
            .unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }
}
