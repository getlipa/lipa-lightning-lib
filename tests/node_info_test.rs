mod setup;

#[cfg(feature = "nigiri")]
mod node_info_test {
    use super::*;

    use bitcoin::secp256k1::PublicKey;
    use uniffi_lipalightninglib::config::NodeAddress;

    #[test]
    fn test_get_node_info() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };
        let node = setup::setup(lsp_node).unwrap();
        let node_info = node.get_node_info();

        assert!(
            PublicKey::from_slice(&*node_info.node_pubkey).is_ok(),
            "Node public key is not valid"
        );
        assert!(
            node_info.num_channels >= node_info.num_usable_channels,
            "Number of channels must be greater or equal to number of usable channels"
        );
        assert!(
            node.get_node_info().local_balance_msat < 21_000_000 * 100_000_000,
            "Node must not hold more than 21 million BTC"
        );
    }
}
