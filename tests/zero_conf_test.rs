mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod zero_conf_test {
    use super::*;
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::nigiri::NodeInstance;
    use crate::setup::{nigiri, NodeHandle};
    use uniffi_lipalightninglib::config::NodeAddress;

    const HALF_SEC: Duration = Duration::from_millis(500);

    #[test]
    fn test_zero_conf_channel_is_usable_without_confirmations() {
        let node_handle = setup_with(NodeInstance::LspdLnd);

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        nigiri::lspd_lnd_open_zero_conf_channel(&node_id).unwrap();

        sleep(Duration::from_secs(5));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }

    #[test]
    fn test_zero_conf_channel_only_accepted_from_lsp() {
        let node_handle = setup_with(NodeInstance::NigiriLnd);

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        let lspd_lnd_node_info = nigiri::query_lnd_node_info(NodeInstance::LspdLnd).unwrap();
        node.connect(&NodeAddress {
            pub_key: lspd_lnd_node_info.pub_key,
            address: "127.0.0.1:9739".to_string(),
        })
        .unwrap();

        assert_eq!(node.get_node_info().num_peers, 2);

        let open_channel_result = nigiri::lspd_lnd_open_zero_conf_channel(&node_id);
        assert!(open_channel_result.is_err());
        assert!(open_channel_result.err().unwrap().contains("disconnected"));
    }

    fn setup_with(node: NodeInstance) -> NodeHandle {
        nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_node_info(node).unwrap();
        let address = match node {
            NodeInstance::NigiriLnd => "127.0.0.1:9735".to_string(),
            NodeInstance::LspdLnd => "127.0.0.1:9739".to_string(),
        };
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address,
        };

        let node_handle = NodeHandle::new(lsp_node);

        fund_lnd_nodes();

        node_handle
    }

    fn fund_lnd_nodes() {
        nigiri::try_cmd_repeatedly(nigiri::fund_nigiri_lnd_node, 0.5, 10, HALF_SEC).unwrap();

        let address = nigiri::get_lnd_node_address(NodeInstance::LspdLnd).unwrap();

        // TODO: convert `try_cmd_repeatedly()` to Macro so that it can also be used here
        let mut retry_times = 10;
        while nigiri::fund_lspd_lnd_node(0.5, address.clone()).is_err() {
            retry_times -= 1;

            if retry_times == 0 {
                panic!("Failed to fund lspd LND node.");
            }
            sleep(HALF_SEC);
        }
    }
}
