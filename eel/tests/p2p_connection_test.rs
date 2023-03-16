mod setup;
mod setup_env;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod p2p_connection_test {
    use bitcoin::hashes::hex::ToHex;
    use serial_test::file_parallel;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::mocked_storage_node;
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;

    #[test]
    #[file_parallel(key, "/tmp/3l-int-tests-lock")]
    fn test_p2p_connection() {
        nigiri::ensure_environment_running();
        let node = mocked_storage_node().start().unwrap();

        sleep(Duration::from_millis(100));
        assert_eq!(node.get_node_info().num_peers, 1);
        let peers = nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
        assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_p2p_connection_with_unreliable_lsp() {
        nigiri::ensure_environment_running();
        let node = mocked_storage_node().start().unwrap();

        // Test disconnect when LSP is down.
        {
            // Let's shutdown LSPD LND.
            nigiri::pause_lspd();
            sleep(Duration::from_secs(1));

            assert_eq!(node.get_node_info().num_peers, 0);
        }

        // Test reconnect when LSP is back.
        {
            // Now let's start LSPD LND again.
            nigiri::start_lspd();
            nigiri::wait_for_healthy_lspd();
            // TODO: Once reconnect period exposed as a config, config it with a
            //       smaller value to speedup the test.
            sleep(Duration::from_secs(10));
            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }
    }
}
