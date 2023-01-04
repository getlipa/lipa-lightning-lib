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
    use uniffi_lipalightninglib::config::NodeAddress;

    const NIGIRI_LND_ADDR: &str = "127.0.0.1:9735";

    #[test]
    fn test_p2p_connection() {
        setup::nigiri::start();

        let lsp_info = setup::nigiri::query_node_info(NodeInstance::NigiriLnd).unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            host: NIGIRI_LND_ADDR.to_string(),
        };

        let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

        // Test successful p2p connection.
        {
            sleep(Duration::from_secs(100));
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
            // Now let's start Nigiri with LND again
            setup::nigiri::start_lspd();
            setup::nigiri::wait_for_healthy_lspd();
            sleep(Duration::from_secs(10));
            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::LspdLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }
    }
}
