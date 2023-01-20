mod setup;

#[cfg(feature = "nigiri")]
mod node_info_test {
    use bitcoin::secp256k1::PublicKey;
    use serial_test::file_parallel;

    use crate::setup::NodeHandle;

    #[test]
    #[file_parallel(key, "/tmp/3l-int-tests-lock")]
    fn test_get_node_info() {
        let node = NodeHandle::new_with_lsp_setup(false);
        let node = node.start().unwrap();

        let node_info = node.get_node_info();

        assert!(
            PublicKey::from_slice(&*node_info.node_pubkey).is_ok(),
            "Node public key is not valid"
        );
        assert!(
            node_info.channels_info.num_channels >= node_info.channels_info.num_usable_channels,
            "Number of channels must be greater or equal to number of usable channels"
        );
        assert!(
            node_info.channels_info.local_balance_msat < 21_000_000 * 100_000_000,
            "Node must not hold more than 21 million BTC"
        );
    }
}
