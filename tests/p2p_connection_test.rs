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
    const FAULTY_ADDR: &str = "127.0.0.1:9";

    #[test]
    fn test_p2p_connection() {
        setup::nigiri::start();

        // test successful p2p connection
        {
            let lsp_info = setup::nigiri::query_node_info(NodeInstance::NigiriLnd).unwrap();
            let lsp_node = NodeAddress {
                pub_key: lsp_info.pub_key,
                host: NIGIRI_LND_ADDR.to_string(),
            };

            let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::NigiriLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }

        // test failing p2p connection
        {
            let lsp_info = setup::nigiri::query_node_info(NodeInstance::NigiriLnd).unwrap();
            let lsp_node = NodeAddress {
                pub_key: lsp_info.pub_key,
                host: FAULTY_ADDR.to_string(),
            };

            let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

            assert_eq!(node.get_node_info().num_peers, 0);
            assert!(setup::nigiri::list_peers(NodeInstance::NigiriLnd)
                .unwrap()
                .is_empty());
        }

        // test start node while lsp is down
        {
            let lsp_info = setup::nigiri::query_node_info(NodeInstance::NigiriLnd).unwrap();
            let lsp_node = NodeAddress {
                pub_key: lsp_info.pub_key,
                host: NIGIRI_LND_ADDR.to_string(),
            };

            // Let's shutdown LND, while we leave Esplora running (test should not affect chain sync)
            setup::nigiri::pause();
            setup::nigiri::resume_without_ln();

            let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

            assert_eq!(node.get_node_info().num_peers, 0);

            // Now let's start Nigiri with LND again
            setup::nigiri::pause();
            setup::nigiri::resume();

            // Wait for the LDK to connect to the LSP (which should now succeed)
            sleep(Duration::from_millis(1500));

            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::NigiriLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }

        // test flaky p2p connection
        {
            let lsp_info = setup::nigiri::query_node_info(NodeInstance::NigiriLnd).unwrap();
            let lsp_node = NodeAddress {
                pub_key: lsp_info.pub_key,
                host: NIGIRI_LND_ADDR.to_string(),
            };
            let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::NigiriLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));

            setup::nigiri::pause();

            // Wait for LDK to register the lost connection, as well as for the LipaNode to attempt to reconnect again, which should fail
            sleep(Duration::from_millis(1500));

            assert_eq!(node.get_node_info().num_peers, 0);

            setup::nigiri::resume();

            // Wait for the LDK to reconnect to the LSP
            sleep(Duration::from_millis(1500));

            assert_eq!(node.get_node_info().num_peers, 1);
            let peers = setup::nigiri::list_peers(NodeInstance::NigiriLnd).unwrap();
            assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));
        }

        // test stop node while lsp is down
        {
            let lsp_info = setup::nigiri::query_node_info(NodeInstance::NigiriLnd).unwrap();
            let lsp_node = NodeAddress {
                pub_key: lsp_info.pub_key,
                host: NIGIRI_LND_ADDR.to_string(),
            };

            {
                let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

                assert_eq!(node.get_node_info().num_peers, 1);
                let peers = setup::nigiri::list_peers(NodeInstance::NigiriLnd).unwrap();
                assert!(peers.contains(&node.get_node_info().node_pubkey.to_hex()));

                setup::nigiri::stop();
                sleep(Duration::from_secs(5)); // Wait for the LDK to disconnect from the LSP
            }
        }
    }
}
