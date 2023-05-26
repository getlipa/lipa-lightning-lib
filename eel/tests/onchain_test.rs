mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod onchain_test {
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{mocked_storage_node, setup_outbound_capacity};
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_force_close_from_peer() {
        nigiri::setup_environment_with_lsp();
        // Set up a channel and receive 50k sats
        let node = mocked_storage_node().start_or_panic();
        let funding_txid = setup_outbound_capacity(&node);

        assert_eq!(node.get_onchain_balance().unwrap().confirmed, 0);

        nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, funding_txid).unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 6);

        wait_for_eq!(node.get_node_info().channels_info.num_channels, 0);
        wait_for_eq!(nigiri::get_number_of_txs_in_mempool(), Ok::<u64, String>(1));

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        wait_for!(node.get_onchain_balance().unwrap().confirmed > 0);
        // We get on-chain balance but it's less than the 50k sats due to
        //      the need of an additional output spending tx.
        assert!(
            node.get_onchain_balance().unwrap().confirmed > 0
                && node.get_onchain_balance().unwrap().confirmed < 50_000
        );
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_coop_close_from_peer() {
        nigiri::setup_environment_with_lsp();
        let node = mocked_storage_node().start_or_panic();
        // Set up a channel and receive 50k sats
        let funding_txid = setup_outbound_capacity(&node);

        assert_eq!(node.get_onchain_balance().unwrap().confirmed, 0);

        nigiri::lnd_node_coop_close_channel(NodeInstance::LspdLnd, funding_txid).unwrap();

        wait_for_eq!(node.get_node_info().channels_info.num_channels, 0);
        wait_for_eq!(nigiri::get_number_of_txs_in_mempool(), Ok::<u64, String>(1));

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        wait_for_eq!(node.get_onchain_balance().unwrap().confirmed, 50_000);
    }
}
