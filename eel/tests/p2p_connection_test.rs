mod setup;
mod setup_env;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod p2p_connection_test {
    use bitcoin::hashes::hex::ToHex;
    use eel::errors::RuntimeErrorCode;
    use perro::runtime_error;
    use serial_test::{file_parallel, file_serial};

    use crate::setup::mocked_storage_node;
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::{wait_for, wait_for_ok};

    #[test]
    #[file_parallel(key, "/tmp/3l-int-tests-lock")]
    fn test_p2p_connection() {
        nigiri::ensure_environment_running();
        let node = mocked_storage_node().start_or_panic();

        wait_for!(!node.get_node_info().peers.is_empty());
        let peers = nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
        assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_p2p_connection_with_unreliable_lsp() {
        // Start the node when lspd isn't available
        nigiri::pause_lspd();
        let node = mocked_storage_node().start_or_panic();

        assert_eq!(
            node.query_lsp_fee(),
            Err(runtime_error(
                RuntimeErrorCode::LspServiceUnavailable,
                "Failed to get LSP info"
            ))
        );
        assert!(node.get_node_info().peers.is_empty());

        // Test reconnect when LSP is back.
        nigiri::start_lspd();
        nigiri::ensure_environment_running();
        nigiri::wait_for_healthy_lspd();
        wait_for!(!node.get_node_info().peers.is_empty());
        let peers = nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
        assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        wait_for_ok!(node.query_lsp_fee());
    }
}
