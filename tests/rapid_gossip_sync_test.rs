mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other
//          because they are manipulating their environment:
//          cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod zero_conf_test {
    use crate::setup::nigiri::{wait_for_new_channel_to_confirm, NodeInstance};
    use crate::setup::{nigiri, NodeHandle};
    use crate::try_cmd_repeatedly;
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;
    use uniffi_lipalightninglib::LightningNode;

    const HUNDRED_K_SATS: u64 = 100_000_000;
    const ONE_K_SATS: u64 = 1_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const LSPD_LND_HOST: &str = "lspd-lnd";
    const LSPD_LND_PORT: u16 = 9739;

    #[test]
    fn test_update_from_0_and_partial_update() {
        let node_handle = NodeHandle::new_with_lsp_rgs_setup();

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
        nigiri::cln_node_open_pub_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        wait_for_new_channel_to_confirm(NodeInstance::LspdLnd, &lipa_node_id);
        wait_for_new_channel_to_confirm(NodeInstance::NigiriCln, &lspd_node_id);

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);

        assert!(node.get_node_info().channels_info.inbound_capacity_msat > 2 * HUNDRED_K_SATS);

        // Pay from NigiriCln to 3L to create outbound liquidity
        let invoice_cln = node
            .create_invoice(HUNDRED_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice_cln.starts_with("lnbc"));

        nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice_cln).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            HUNDRED_K_SATS
        );
        // TODO: figure out why the following sleep is needed - the assert that follows fails otherwise
        sleep(Duration::from_secs(10));
        assert!(node.get_node_info().channels_info.outbound_capacity_msat > 0);

        // wait for the RGS server to learn about the new channels (100 seconds isn't enough)
        sleep(Duration::from_secs(150));

        node.sync_graph().unwrap();

        send_payment_flow(&node, NodeInstance::NigiriCln, ONE_K_SATS);

        // Create new channel - the 3L node will have to learn about it in a partial sync
        nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        wait_for_new_channel_to_confirm(NodeInstance::NigiriLnd, &lspd_node_id);

        // Pay from NigiriLnd to 3L to create outbound liquidity (LspdLnd -> NigiriLnd)
        let invoice_lnd = node
            .create_invoice(HUNDRED_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice_lnd.starts_with("lnbc"));

        nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice_lnd).unwrap();

        // wait for the RGS server to learn about the new channels (100 seconds isn't enough)
        sleep(Duration::from_secs(150));

        node.sync_graph().unwrap();

        send_payment_flow(&node, NodeInstance::NigiriLnd, ONE_K_SATS);
    }

    fn send_payment_flow(node: &LightningNode, target: NodeInstance, amount_msat: u64) {
        let invoice = nigiri::issue_invoice(target, "test", amount_msat, 3600).unwrap();
        let initial_balance = nigiri::query_node_balance(target).unwrap();
        node.pay_invoice(invoice).unwrap();
        sleep(Duration::from_secs(2));
        let final_balance = nigiri::query_node_balance(target).unwrap();
        assert_eq!(final_balance - initial_balance, amount_msat);
    }
}
