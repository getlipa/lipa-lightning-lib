mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod node_info_test {
    use serial_test::file_serial;

    use crate::setup::mocked_storage_node;
    use crate::setup_env::nigiri;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_get_node_info() {
        nigiri::setup_environment_with_lsp();

        let node = mocked_storage_node().start_or_panic();
        let node_info = node.get_node_info();

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
