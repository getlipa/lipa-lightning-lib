mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod p2p_connection_test {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::NodeHandle;
    use uniffi_lipalightninglib::config::NodeAddress;

    const NIGIRI_LND_ADDR: &str = "127.0.0.1:9735";
    const FAULTY_ADDR: &str = "127.0.0.1:9";

    #[test]
    fn test_successful_p2p_connection() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: NIGIRI_LND_ADDR.to_string(),
        };

        let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

        assert_eq!(node.get_node_info().num_peers, 1);
        assert!(node.connected_to_node(&lsp_node));
    }

    #[test]
    fn test_failing_p2p_connection() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: FAULTY_ADDR.to_string(),
        };

        let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

        assert_eq!(node.get_node_info().num_peers, 0);
        assert!(!node.connected_to_node(&lsp_node));
    }

    #[test]
    fn test_start_node_while_lsp_is_down() {
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: NIGIRI_LND_ADDR.to_string(),
        };

        // Let's shutdown LND, while we leave Esplora running (test should not affect chain sync)
        setup::nigiri::pause();
        setup::nigiri::resume_without_ln();

        let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

        // Wait for the LDK to connect to the LSP (which should fail)
        sleep(Duration::from_millis(1500));

        assert_eq!(node.get_node_info().num_peers, 0);
        assert!(!node.connected_to_node(&lsp_node));

        // Now let's start Nigiri with LND again
        setup::nigiri::pause();
        setup::nigiri::resume();

        // Wait for the LDK to connect to the LSP (which should now succeed)
        sleep(Duration::from_millis(1500));

        assert_eq!(node.get_node_info().num_peers, 1);
        assert!(node.connected_to_node(&lsp_node));
    }

    #[test]
    fn test_stop_node_while_lsp_is_down() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: NIGIRI_LND_ADDR.to_string(),
        };

        {
            let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

            assert_eq!(node.get_node_info().num_peers, 1);
            assert!(node.connected_to_node(&lsp_node));

            setup::nigiri::stop();
            sleep(Duration::from_secs(5)); // Wait for the LDK to disconnect from the LSP
        }

        // node has been dropped and thus shutdown.
    }

    #[test]
    fn test_flaky_p2p_connection() {
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: NIGIRI_LND_ADDR.to_string(),
        };
        let node = NodeHandle::new(lsp_node.clone()).start().unwrap();

        assert_eq!(node.get_node_info().num_peers, 1);
        assert!(node.connected_to_node(&lsp_node));

        setup::nigiri::pause();

        // Wait for LDK to register the lost connection, as well as for the LipaNode to attempt to reconnect again, which should fail
        sleep(Duration::from_millis(1500));

        assert_eq!(node.get_node_info().num_peers, 0);
        assert!(!node.connected_to_node(&lsp_node));

        setup::nigiri::resume();

        // Wait for the LDK to reconnect to the LSP
        sleep(Duration::from_millis(1500));

        assert_eq!(node.get_node_info().num_peers, 1);
        assert!(node.connected_to_node(&lsp_node));
    }
}
