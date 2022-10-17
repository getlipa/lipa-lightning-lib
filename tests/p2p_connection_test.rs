mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod p2p_connection_test {
    use super::*;

    use uniffi_lipalightninglib::config::NodeAddress;

    #[test]
    fn test_successful_p2p_connection() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };

        let node = setup::setup(lsp_node).unwrap();
        assert_eq!(node.get_node_info().num_peers, 1)
    }

    #[test]
    fn test_failing_p2p_connection() {
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9".to_string(),
        };
        assert!(setup::setup(lsp_node).is_err());
    }

    #[test]
    fn test_flaky_p2p_connection() {
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };
        let node = setup::setup(lsp_node).unwrap();
        assert_eq!(node.get_node_info().num_peers, 1);

        setup::nigiri::stop();
        assert_eq!(node.get_node_info().num_peers, 0);

        // TODO: Test reconnecting (not implemented yet).
    }
}
