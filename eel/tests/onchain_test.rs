mod setup;
mod setup_env;

#[cfg(feature = "nigiri")]
mod onchain_test {
    use bdk::blockchain::EsploraBlockchain;
    use bdk::database::MemoryDatabase;
    use bdk::{Balance, SyncOptions};
    use bitcoin::util::bip32::ChildNumber;
    use secp256k1::SECP256K1;
    use serial_test::file_serial;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::{mocked_storage_node, setup_outbound_capacity};
    use crate::setup_env::config::get_testing_config;
    use crate::setup_env::nigiri;
    use crate::setup_env::nigiri::NodeInstance;
    use crate::{try_cmd_repeatedly, wait_for, wait_for_eq};

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    pub struct Wallet {
        blockchain: EsploraBlockchain,
        inner: bdk::Wallet<MemoryDatabase>,
    }

    impl Wallet {
        pub(crate) fn new() -> Self {
            let config = get_testing_config();

            let xprv =
                bitcoin::util::bip32::ExtendedPrivKey::new_master(config.network, &config.seed)
                    .unwrap();
            let destination_xprv = xprv
                .ckd_priv(SECP256K1, ChildNumber::from_hardened_idx(1).unwrap())
                .unwrap();
            let shutdown_xprv = xprv
                .ckd_priv(SECP256K1, ChildNumber::from_hardened_idx(2).unwrap())
                .unwrap();

            let descriptor_destination = format!("wpkh({})", destination_xprv);
            let descriptor_shutdown = format!("wpkh({})", shutdown_xprv);

            let db = MemoryDatabase::default();
            let wallet = bdk::Wallet::new(
                &descriptor_destination,
                Some(&descriptor_shutdown),
                config.network,
                db,
            )
            .unwrap();

            let esplora_client = bdk::esplora_client::Builder::new(&config.esplora_api_url)
                .timeout(30)
                .build_blocking()
                .unwrap();

            let blockchain = EsploraBlockchain::from_client(esplora_client, 20).with_concurrency(8);

            Self {
                blockchain,
                inner: wallet,
            }
        }

        fn sync(&self) {
            self.inner
                .sync(&self.blockchain, SyncOptions::default())
                .unwrap();
        }

        pub(crate) fn get_onchain_balance(&self) -> Balance {
            self.sync();
            self.inner.get_balance().unwrap()
        }
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_force_close_from_peer() {
        nigiri::setup_environment_with_lsp();
        // Set up a channel and receive 50k sats
        let node = mocked_storage_node().start_or_panic();
        let wallet = Wallet::new();
        let funding_txid = setup_outbound_capacity(&node);

        assert_eq!(wallet.get_onchain_balance().confirmed, 0);

        nigiri::lnd_node_force_close_channel(NodeInstance::LspdLnd, funding_txid).unwrap();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 6);

        wait_for_eq!(node.get_node_info().channels_info.num_channels, 0);
        wait_for_eq!(nigiri::get_number_of_txs_in_mempool(), Ok::<u64, String>(1));

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        wait_for!(wallet.get_onchain_balance().confirmed > 0);
        // We get on-chain balance but it's less than the 50k sats due to
        //      the need of an additional output spending tx.
        assert!(
            wallet.get_onchain_balance().confirmed > 0
                && wallet.get_onchain_balance().confirmed < 50_000
        );
    }

    #[test]
    #[file_serial(key, "/tmp/3l-int-tests-lock")]
    fn test_coop_close_from_peer() {
        nigiri::setup_environment_with_lsp();
        let node = mocked_storage_node().start_or_panic();
        let wallet = Wallet::new();
        // Set up a channel and receive 50k sats
        let funding_txid = setup_outbound_capacity(&node);

        assert_eq!(wallet.get_onchain_balance().confirmed, 0);

        nigiri::lnd_node_coop_close_channel(NodeInstance::LspdLnd, funding_txid).unwrap();

        wait_for_eq!(node.get_node_info().channels_info.num_channels, 0);
        wait_for_eq!(nigiri::get_number_of_txs_in_mempool(), Ok::<u64, String>(1));

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 1);

        wait_for_eq!(wallet.get_onchain_balance().confirmed, 50_000);
    }
}
