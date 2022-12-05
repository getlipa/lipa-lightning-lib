mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other
//      because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod zero_conf_test {
    use crate::setup::nigiri::NodeInstance;
    use crate::setup::{nigiri, NodeHandle};
    use crate::try_cmd_repeatedly;
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    const TWENTY_K_SATS: u64 = 20_000_000;
    const HUNDRED_K_SATS: u64 = 100_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const LSPD_LND_HOST: &str = "lspd-lnd";
    const LSPD_LND_PORT: u16 = 9739;

    #[test]
    fn test_update_from_0() {
        let node_handle = NodeHandle::new_with_lsp_rgs_setup();

        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        // CONNECT NigiriLnd -> LspdLnd -> 3L + NigiriCln -> LspdLnd
        nigiri::node_connect(
            NodeInstance::NigiriLnd,
            &lspd_node_id,
            LSPD_LND_HOST,
            LSPD_LND_PORT,
        )
        .unwrap();
        nigiri::node_connect(
            NodeInstance::NigiriCln,
            &lspd_node_id,
            LSPD_LND_HOST,
            LSPD_LND_PORT,
        )
        .unwrap();
        sleep(Duration::from_secs(20));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        nigiri::cln_node_open_pub_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TWENTY_K_SATS);

        // Pay from NigiriLnd and NigiriCln to 3L to create outbound liquidity
        let invoice_lnd = node
            .create_invoice(HUNDRED_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice_lnd.starts_with("lnbc"));
        let invoice_cln = node
            .create_invoice(HUNDRED_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice_cln.starts_with("lnbc"));

        sleep(Duration::from_secs(100)); // wait for super lazy cln to consider its channels active

        nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice_lnd).unwrap();
        nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice_cln).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            2 * HUNDRED_K_SATS
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat > 0);

        sleep(Duration::from_secs(250));

        node.sync_graph().unwrap();

        // TODO: get a route or make a payment towards NigiriLnd and NigiriCln
    }
}
