mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod zero_conf_test {
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::nigiri::{fund_lnd_node, NodeInstance};
    use crate::setup::{nigiri, NodeHandle};
    use uniffi_lipalightninglib::config::NodeAddress;

    #[test]
    fn test_zero_conf_channel_is_usable_without_confirmations() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        const TEN_SATS: u64 = 10_000;

        // With no channels, 10 sats invoice is to small to cover channel
        // opening fees.
        let invoice = node.create_invoice(TEN_SATS, "test".to_string());
        assert_eq!(
	    invoice.unwrap_err().to_string(),
	    "PermanentFailure: Failed to register payment: InvalidInput: Payment amount must be bigger than fees");
        let invoice = node.create_invoice(20_000_000, "test".to_string());
        assert!(invoice.unwrap().starts_with("lnbc"));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, true).unwrap();

        sleep(Duration::from_secs(5));

        // With a channel, 10 sats invoice is perfectly fine.
        let invoice = node.create_invoice(TEN_SATS, "test".to_string());
        assert!(invoice.unwrap().starts_with("lnbc"));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }

    fn setup() -> NodeHandle {
        nigiri::start();
        let lsp_info = nigiri::query_lnd_node_info(NodeInstance::LspdLnd).unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9739".to_string(),
        };

        let node_handle = NodeHandle::new(lsp_node);

        fund_lnd_node(NodeInstance::LspdLnd, 0.5);

        node_handle
    }
}
