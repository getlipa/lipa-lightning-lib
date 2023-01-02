mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod receiving_payments_test {
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;
    use uniffi_lipalightninglib::LightningNode;

    use crate::setup::nigiri::NodeInstance;
    use crate::setup::{nigiri, NodeHandle};
    use crate::try_cmd_repeatedly;

    const ONE_SAT: u64 = 1_000;
    const ONE_K_SATS: u64 = 1_000_000;
    const TWO_K_SATS: u64 = 2_000_000;
    const TEN_K_SATS: u64 = 10_000_000;
    const TWENTY_K_SATS: u64 = 20_000_000;
    const FIVE_HUNDRED_K_SATS: u64 = 500_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const LSPD_LND_HOST: &str = "lspd-lnd";
    const LSPD_LND_PORT: u16 = 9739;

    #[test]
    // Test receiving an invoice on a node that does not have any channel yet
    // resp, the channel opening is part of the payment process.
    fn receive_payment_with_jit_channel_fresh_node() {
        let node_handle = NodeHandle::new_with_lsp_setup();

        let node = node_handle.start().unwrap();
        assert_eq!(node.get_node_info().num_peers, 1);

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);

        nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        run_jit_channel_open_flow(
            &node,
            NodeInstance::NigiriLnd,
            TWO_K_SATS + ONE_SAT,
            TWO_K_SATS,
        );
    }

    #[test]
    fn receive_payment_with_jit_channel_existing_channels() {
        let node = nigiri::initiate_node_with_channel(NodeInstance::LspdLnd);
        run_payment_flow(&node, NodeInstance::LspdLnd, FIVE_HUNDRED_K_SATS);

        // We have a 1M sat channel and have received a 0.5M payment. Another 0.5M payment is not
        // possible due to channel reserves. A new channel with 0.6M size should be created

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);

        nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        let initial_num_channels = node.get_node_info().channels_info.num_usable_channels;

        run_jit_channel_open_flow(
            &node,
            NodeInstance::NigiriLnd,
            FIVE_HUNDRED_K_SATS,
            TWO_K_SATS,
        );

        assert_eq!(
            node.get_node_info().channels_info.num_usable_channels,
            initial_num_channels + 1
        );
    }

    #[test]
    // Test receiving an invoice on a node that already has an open channel
    fn receive_payment_on_established_node() {
        let node = nigiri::initiate_node_with_channel(NodeInstance::LspdLnd);
        run_payment_flow(&node, NodeInstance::LspdLnd, TWENTY_K_SATS);
    }

    #[test]
    // The difference between sending 1_000 sats and 20_000 sats (test case receive_payment_on_established_node)
    // is that receiving 1_000 sats creates a dust-HTLC, while receiving 20_000 sats does not.
    // A dust-HTLC is an HTLC that is too small to be worth the fees to settle it.
    fn receive_dust_htlc_payment_1k() {
        let node = nigiri::initiate_node_with_channel(NodeInstance::LspdLnd);
        run_payment_flow(&node, NodeInstance::LspdLnd, ONE_K_SATS);
    }

    #[test]
    fn receive_dust_htlc_payment_10k() {
        let node = nigiri::initiate_node_with_channel(NodeInstance::LspdLnd);
        run_payment_flow(&node, NodeInstance::LspdLnd, TEN_K_SATS);
    }

    #[test]
    fn receive_multiple_payments() {
        let amt_of_payments = 10;
        let node = nigiri::initiate_node_with_channel(NodeInstance::LspdLnd);
        assert_channel_ready(&node, TWENTY_K_SATS * amt_of_payments);

        for i in 1..=amt_of_payments {
            let invoice = issue_invoice(&node, TWENTY_K_SATS);

            nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();
            assert_eq!(
                node.get_node_info().channels_info.local_balance_msat,
                TWENTY_K_SATS * i
            );
        }

        assert_payment_received(&node, TWENTY_K_SATS * amt_of_payments);
    }

    #[test]
    // Tests correctness of the routing hint within the invoice
    fn receive_payment_from_lnd_with_hop() {
        let node_handle = NodeHandle::new_with_lsp_setup();

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);
        sleep(Duration::from_secs(1));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        run_payment_flow(&node, NodeInstance::NigiriLnd, TWENTY_K_SATS);
    }

    #[test]
    // Tests correctness of the routing hint within the invoice
    fn receive_payment_from_cln_with_hop() {
        let node_handle = NodeHandle::new_with_lsp_setup();

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        connect_node_to_lsp(NodeInstance::NigiriCln, &lspd_node_id);
        sleep(Duration::from_secs(20));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::cln_node_open_pub_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(110)); // wait for super lazy cln to consider its channels active

        run_payment_flow(&node, NodeInstance::NigiriCln, TWENTY_K_SATS);
    }

    #[test]
    fn receive_multiple_payments_for_same_invoice() {
        let node_handle = NodeHandle::new_with_lsp_setup();

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        let lspd_node_id = nigiri::query_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        connect_node_to_lsp(NodeInstance::NigiriLnd, &lspd_node_id);
        connect_node_to_lsp(NodeInstance::NigiriCln, &lspd_node_id);
        sleep(Duration::from_secs(20));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        nigiri::cln_node_open_pub_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(110)); // wait for super lazy cln to consider its channels active

        assert_channel_ready(&node, TWENTY_K_SATS * 3);
        let invoice = issue_invoice(&node, TWENTY_K_SATS);

        nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();
        nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice).unwrap();
        nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice).unwrap();

        assert_payment_received(&node, TWENTY_K_SATS * 3);
    }

    fn run_jit_channel_open_flow(
        node: &LightningNode,
        paying_node: NodeInstance,
        payment_amount: u64,
        lsp_fee: u64,
    ) {
        let initial_balance = node.get_node_info().channels_info.local_balance_msat;

        let invoice = issue_invoice(&node, payment_amount);

        nigiri::pay_invoice(paying_node, &invoice).unwrap();

        assert_payment_received(&node, initial_balance + payment_amount - lsp_fee);
    }

    fn run_payment_flow(node: &LightningNode, paying_node: NodeInstance, payment_amount: u64) {
        let initial_balance = node.get_node_info().channels_info.local_balance_msat;

        assert_channel_ready(&node, payment_amount);
        let invoice = issue_invoice(&node, payment_amount);

        nigiri::pay_invoice(paying_node, &invoice).unwrap();

        assert_payment_received(&node, initial_balance + payment_amount);
    }

    fn assert_channel_ready(node: &LightningNode, payment_amount: u64) {
        assert!(node.get_node_info().channels_info.num_channels > 0);
        assert!(node.get_node_info().channels_info.num_usable_channels > 0);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > payment_amount);
    }

    fn assert_payment_received(node: &LightningNode, expected_balance: u64) {
        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            expected_balance
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat < expected_balance);
        // because of channel reserves
    }

    fn issue_invoice(node: &LightningNode, payment_amount: u64) -> String {
        let invoice = node
            .create_invoice(payment_amount, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        invoice
    }

    fn connect_node_to_lsp(node: NodeInstance, lsp_node_id: &str) {
        nigiri::node_connect(node, lsp_node_id, LSPD_LND_HOST, LSPD_LND_PORT).unwrap();
    }
}
