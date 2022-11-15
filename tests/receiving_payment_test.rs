mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod receiving_payments_test {
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use crate::setup::nigiri::{fund_node, NodeInstance};
    use crate::setup::{nigiri, NodeHandle};
    use crate::try_cmd_repeatedly;
    use uniffi_lipalightninglib::config::NodeAddress;

    const THOUSAND_SATS: u64 = 1_000_000;
    const TEN_K_SATS: u64 = 10_000_000;
    const TWENTY_K_SATS: u64 = 20_000_000;
    const MILLION_SATS: u64 = 1_000_000_000;

    const HALF_SEC: Duration = Duration::from_millis(500);
    const N_RETRIES: u8 = 10;

    const LSPD_LND_HOST: &str = "lspd-lnd";
    const LSPD_LND_PORT: u16 = 9739;

    #[test]
    // Test receiving an invoice on a node that does not have any channel yet
    // resp, the channel opening is part of the payment process.
    fn receive_payment_on_fresh_node() {
        // todo: as soon as LSPD functionality is implemented
    }

    #[test]
    // Test receiving an invoice on a node that already has an open channel
    fn receive_payment_on_established_node() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TWENTY_K_SATS);

        let invoice = node
            .create_invoice(TWENTY_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            TWENTY_K_SATS
        );
        assert_eq!(node.get_node_info().channels_info.outbound_capacity_msat, 0); // because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat < MILLION_SATS - TWENTY_K_SATS
        ); // smaller instead of equal because of channel reserves
    }

    #[test]
    // The difference between sending 1_000 sats and 20_000 sats (test case receive_payment_on_established_node)
    // is that receiving 1_000 sats creates a dust-HTLC, while receiving 20_000 sats does not.
    // A dust-HTLC is an HTLC that is too small to be worth the fees to settle it.
    fn receive_dust_htlc_payment() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > THOUSAND_SATS);

        let invoice = node
            .create_invoice(THOUSAND_SATS, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            THOUSAND_SATS
        );
        assert_eq!(node.get_node_info().channels_info.outbound_capacity_msat, 0); // because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat < MILLION_SATS - THOUSAND_SATS
        ); // smaller instead of equal because of channel reserves
    }

    #[test]
    // Todo remove this test, once the bug is fixed
    // This is kind of the opposite of a test.
    // Instead of testing whether a feature *works*, it is here to test whether a bug still exists.
    // This serves kind of as a reminder, as well as a description and proof of the bug.
    // In case the bug gets fixed as a byproduct, for example through updating dependencies,
    // this test should be removed, and the issues in the project management tools should be resolved.
    fn dust_bug_still_exists() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TEN_K_SATS);

        let invoice = node.create_invoice(TEN_K_SATS, "test".to_string()).unwrap();
        assert!(invoice.starts_with("lnbc"));

        let result = nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice);
        assert!(result.is_err());
    }

    #[test]
    fn receive_multiple_payments() {
        let amt_of_payments = 10;
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let node_id = node.get_node_info().node_pubkey.to_hex();

        assert_eq!(node.get_node_info().num_peers, 1);

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat
                > TWENTY_K_SATS * amt_of_payments
        );

        for i in 1..=amt_of_payments {
            let invoice = node
                .create_invoice(TWENTY_K_SATS, "test".to_string())
                .unwrap();
            assert!(invoice.starts_with("lnbc"));

            nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();
            assert_eq!(
                node.get_node_info().channels_info.local_balance_msat,
                TWENTY_K_SATS * i
            );
        }

        assert!(node.get_node_info().channels_info.outbound_capacity_msat > 0);
    }

    #[test]
    // Tests correctness of the routing hint within the invoice
    fn receive_payment_from_lnd_with_hop() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        fund_node(NodeInstance::NigiriLnd, 0.5);

        let lspd_node_id = nigiri::query_lnd_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        nigiri::node_connect(
            NodeInstance::NigiriLnd,
            &lspd_node_id,
            LSPD_LND_HOST,
            LSPD_LND_PORT,
        )
        .unwrap();
        sleep(Duration::from_secs(1));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::lnd_node_open_pub_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TWENTY_K_SATS);

        let invoice = node
            .create_invoice(TWENTY_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            TWENTY_K_SATS
        );
        assert_eq!(node.get_node_info().channels_info.outbound_capacity_msat, 0); // because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat < MILLION_SATS - TWENTY_K_SATS
        ); // smaller instead of equal because of channel reserves
    }

    #[test]
    // Tests correctness of the routing hint within the invoice
    fn receive_payment_from_cln_with_hop() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        fund_node(NodeInstance::NigiriCln, 0.5);

        let lspd_node_id = nigiri::query_lnd_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

        nigiri::node_connect(
            NodeInstance::NigiriCln,
            &lspd_node_id,
            LSPD_LND_HOST,
            LSPD_LND_PORT,
        )
        .unwrap();
        sleep(Duration::from_secs(20));

        nigiri::lnd_node_open_channel(NodeInstance::LspdLnd, &lipa_node_id, false).unwrap();
        nigiri::cln_node_open_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TWENTY_K_SATS);

        let invoice = node
            .create_invoice(TWENTY_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        sleep(Duration::from_secs(100)); // wait for super lazy cln to consider its channels active

        nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            TWENTY_K_SATS
        );
        assert_eq!(node.get_node_info().channels_info.outbound_capacity_msat, 0); // because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat < MILLION_SATS - TWENTY_K_SATS
        );
    }

    #[test]
    fn receive_multiple_payments_for_same_invoice() {
        let node_handle = setup();

        let node = node_handle.start().unwrap();
        let lipa_node_id = node.get_node_info().node_pubkey.to_hex();
        assert_eq!(node.get_node_info().num_peers, 1);

        fund_node(NodeInstance::NigiriLnd, 0.5);
        fund_node(NodeInstance::NigiriCln, 0.5);

        let lspd_node_id = nigiri::query_lnd_node_info(NodeInstance::LspdLnd)
            .unwrap()
            .pub_key;

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
        nigiri::lnd_node_open_channel(NodeInstance::NigiriLnd, &lspd_node_id, false).unwrap();
        nigiri::cln_node_open_channel(NodeInstance::NigiriCln, &lspd_node_id).unwrap();
        try_cmd_repeatedly!(nigiri::mine_blocks, N_RETRIES, HALF_SEC, 10);
        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().channels_info.num_channels, 1);
        assert_eq!(node.get_node_info().channels_info.num_usable_channels, 1);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > TWENTY_K_SATS * 3);

        let invoice = node
            .create_invoice(TWENTY_K_SATS, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        sleep(Duration::from_secs(100)); // wait for super lazy cln to consider its channels active

        nigiri::lnd_pay_invoice(NodeInstance::LspdLnd, &invoice).unwrap();
        nigiri::lnd_pay_invoice(NodeInstance::NigiriLnd, &invoice).unwrap();
        nigiri::cln_pay_invoice(NodeInstance::NigiriCln, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            TWENTY_K_SATS * 3
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat < TWENTY_K_SATS * 3); // smaller instead of equal because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat
                < MILLION_SATS - TWENTY_K_SATS * 3
        ); // smaller instead of equal because of channel reserves
    }

    // todo move to setup
    fn setup() -> NodeHandle {
        nigiri::start();
        let lsp_info = nigiri::query_lnd_node_info(NodeInstance::LspdLnd).unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9739".to_string(),
        };

        let node_handle = NodeHandle::new(lsp_node);
        fund_node(NodeInstance::LspdLnd, 0.5);

        node_handle
    }
}
