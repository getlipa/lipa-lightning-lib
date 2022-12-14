mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod p2p_connection_test {
    use super::*;
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::nigiri::NodeInstance;
    use crate::setup::NodeHandle;

    #[test]
    fn test_p2p_connection() {
        setup::nigiri::start();

        let node = NodeHandle::new().start().unwrap();

        // Test successful p2p connection.
        {
            sleep(Duration::from_millis(100));
            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }

        // Test disconnect when LSP is down.
        {
            // Let's shutdown LSPD LND.
            setup::nigiri::pause_lspd();
            sleep(Duration::from_secs(1));

            assert_eq!(node.get_node_info().num_peers, 0);
        }

        // Test reconnect when LSP is back.
        {
            // Now let's start LSPD LND again.
            setup::nigiri::start_lspd();
            setup::nigiri::wait_for_healthy_lspd();
            // TODO: Once reconnect period exposed as a config, config it with a
            //       smaller value to speedup the test.
            sleep(Duration::from_secs(10));
            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }
    }
}
