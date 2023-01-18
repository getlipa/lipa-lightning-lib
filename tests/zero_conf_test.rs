mod setup;

#[cfg(feature = "nigiri")]
mod zero_conf_test {
    use bitcoin::hashes::hex::ToHex;
    use serial_test::file_serial;

    use crate::setup::nigiri::{wait_for_new_channel_to_confirm, NodeInstance};
    use crate::setup::{nigiri, NodeHandle};

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_zero_conf_channel_is_usable_without_confirmations() {
        let node_handle = NodeHandle::new_with_lsp_setup(true);

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        const TEN_SATS: u64 = 10_000;
        const TWENTY_K_SATS: u64 = 20_000_000;

        // With no channels, 10 sats invoice is too small to cover channel
        // opening fees.
        assert!(node.get_node_info().channels_info.inbound_capacity_msat < TEN_SATS);
        let invoice = node.create_invoice(TEN_SATS, "test".to_string());
        assert_eq!(
            invoice.unwrap_err().to_string(),
            "InvalidInput: Payment amount must be higher than lsp fees"
        );
        let invoice = node.create_invoice(TWENTY_K_SATS, "test".to_string());
        assert!(invoice.unwrap().starts_with("lnbc"));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, true).unwrap();
        wait_for_new_channel_to_confirm(NodeInstance::LspdLnd, &node_id);

        // With a channel, 10 sats invoice is perfectly fine.
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TEN_SATS);
        let invoice = node.create_invoice(TEN_SATS, "test".to_string()).unwrap();
        assert!(invoice.starts_with("lnbc"));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

        nigiri::pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            TEN_SATS
        );
    }
}
