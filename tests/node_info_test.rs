#[path = "print_events_handler/mod.rs"]
mod print_events_handler;
mod setup_3l;
#[path = "../eel/tests/setup_env/mod.rs"]
mod setup_env;

#[cfg(feature = "nigiri")]
mod node_info_test {
    use crate::setup_3l::NodeHandle;
    use crate::setup_env::nigiri;
    use std::str::FromStr;

    use bitcoin::secp256k1::PublicKey;
    use serial_test::file_serial;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_get_node_info() {
        nigiri::setup_environment_with_lsp();

        let node = NodeHandle::new().start().unwrap();
        let node_info = node.get_node_info();

        assert!(
            PublicKey::from_str(&*node_info.node_pubkey).is_ok(),
            "Node public key is not valid"
        );
        assert!(
            node_info.channels_info.num_channels >= node_info.channels_info.num_usable_channels,
            "Number of channels must be greater or equal to number of usable channels"
        );
        assert!(
            node_info.channels_info.local_balance.sats < 21_000_000 * 100_000_000,
            "Node must not hold more than 21 million BTC"
        );
    }
}
