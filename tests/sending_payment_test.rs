mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
//      because they are manipulating their environment:
//      cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod sending_payments_test {
    use std::thread::sleep;
    use std::time::{Duration, UNIX_EPOCH};
    use uniffi_lipalightninglib::InvoiceDetails;

    use crate::setup::nigiri::NodeInstance::{LspdLnd, NigiriCln, NigiriLnd};
    use crate::setup::{nigiri, NodeHandle};

    const THOUSAND_SATS: u64 = 1_000_000;

    const SECONDS_IN_AN_HOUR: u64 = 3600;

    const DESCRIPTION_SAMPLE: &str = "Luke, I Am Your Father";

    const REGTEST_INVOICE: &str = "lnbcrt10u1p36gm69pp56haryefc0cvsdc7ucwgnm4j3kul5pjkdlc94vwkju5xktwsvtv6sdpyf36kkefvypyjqstdypvk7atjyprxzargv4eqcqzpgxqrrsssp5e777z0f2g05l99yw8cuvhnq68e7xstulcan5tzvdh4f6642f836q9qyyssqw2g88srqdqrqngzcrzq877hz64sf320kgh5yjwwg7negxeuq909kac33tgheq7re5k7luh6q3xam6jk46p0cepkx89hfdl9g0mx24csqgxhk8x";
    const REGTEST_INVOICE_HASH: &str =
        "d5fa3265387e1906e3dcc3913dd651b73f40cacdfe0b563ad2e50d65ba0c5b35";
    const REGTEST_INVOICE_PAYEE_PUB_KEY: &str =
        "02cc8a9ee52470a08bfe54194cb9b25021bed4c05db6c08118f6d92f97c070b234";
    const REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH: Duration = Duration::from_secs(1671720773);
    const REGTEST_INVOICE_EXPIRY: Duration = Duration::from_secs(SECONDS_IN_AN_HOUR);

    const ONCHAIN_ADDRESS: &str = "bc1qsuhxszxhk7nnzy2888sj66ru7kcwp70jexvd8z";

    const MAINNET_INVOICE: &str = "lnbc10u1p36gu9hpp5dmg5up4kyhkefpxue5smrgucg889esu6zc9vyntnzmqyyr4dycaqdqqcqpjsp5h0n3nc53t2tcm8a9kjpsdgql7ex2c7qrpc0dn4ja9c64adycxx7s9q7sqqqqqqqqqqqqqqqqqqqsqqqqqysgqmqz9gxqyjw5qrzjqwryaup9lh50kkranzgcdnn2fgvx390wgj5jd07rwr3vxeje0glcll63swcqxvlas5qqqqlgqqqqqeqqjqmstdrvfcyq9as46xuu63dfgstehmthqlxg8ljuyqk2z9mxvhjfzh0a6jm53rrgscyd7v0y7dj4zckq69tlsdex0352y89wmvvv0j3gspnku4sz";

    #[test]
    fn invoice_decode_test() {
        let node_handle = NodeHandle::new_with_lsp_setup();

        let node = node_handle.start().unwrap();

        // Test valid hardcoded invoice

        let invoice_details = node.decode_invoice(REGTEST_INVOICE.to_string()).unwrap();

        assert_eq!(invoice_details.payment_hash, REGTEST_INVOICE_HASH);
        assert_eq!(
            invoice_details
                .invoice_timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap(),
            REGTEST_INVOICE_DURATION_FROM_UNIX_EPOCH
        );
        assert_invoice_details(
            invoice_details,
            THOUSAND_SATS,
            DESCRIPTION_SAMPLE,
            REGTEST_INVOICE_EXPIRY,
            REGTEST_INVOICE_PAYEE_PUB_KEY,
        );

        // Test invalid hardcoded invoice (fail to parse)

        let invoice_details_result = node.decode_invoice(ONCHAIN_ADDRESS.to_string());
        assert!(invoice_details_result.is_err());
        assert!(invoice_details_result
            .err()
            .unwrap()
            .to_string()
            .contains("Invalid invoice - parse failure"));

        // Test invalid hardcoded invoice (wrong network)

        let invoice_details_result = node.decode_invoice(MAINNET_INVOICE.to_string());
        assert!(invoice_details_result.is_err());
        assert!(invoice_details_result
            .err()
            .unwrap()
            .to_string()
            .contains("Invalid invoice - network mismatch"));

        // Test invoice from CLN

        let invoice = nigiri::issue_invoice(
            NigiriCln,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            THOUSAND_SATS,
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(NigiriCln).unwrap().pub_key,
        );

        // Test invoice from LspdLND

        let invoice = nigiri::issue_invoice(
            LspdLnd,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            THOUSAND_SATS,
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(LspdLnd).unwrap().pub_key,
        );

        // Test invoice from NigiriLND

        let invoice = nigiri::issue_invoice(
            NigiriLnd,
            DESCRIPTION_SAMPLE,
            THOUSAND_SATS,
            SECONDS_IN_AN_HOUR,
        )
        .unwrap();

        let invoice_details = node.decode_invoice(invoice).unwrap();

        assert_invoice_details(
            invoice_details,
            THOUSAND_SATS,
            DESCRIPTION_SAMPLE,
            Duration::from_secs(SECONDS_IN_AN_HOUR),
            &nigiri::query_node_info(NigiriLnd).unwrap().pub_key,
        );
    }

    const REBALANCE_AMOUNT: u64 = 50_000_000;
    const CHANNEL_SIZE: u64 = 1_000_000_000;
    const PAYMENT_AMOUNT: u64 = 1_000_000;

    #[test]
    fn pay_invoice_direct_peer_test() {
        let node = nigiri::initiate_node_with_channel(LspdLnd);

        assert!(node.get_node_info().channels_info.num_channels > 0);
        assert!(node.get_node_info().channels_info.num_usable_channels > 0);
        assert!(node.get_node_info().channels_info.inbound_capacity_msat > REBALANCE_AMOUNT);

        let invoice = node
            .create_invoice(REBALANCE_AMOUNT, "test".to_string())
            .unwrap();
        assert!(invoice.starts_with("lnbc"));

        nigiri::pay_invoice(LspdLnd, &invoice).unwrap();

        assert_eq!(
            node.get_node_info().channels_info.local_balance_msat,
            REBALANCE_AMOUNT
        );
        assert!(node.get_node_info().channels_info.outbound_capacity_msat < REBALANCE_AMOUNT); // because of channel reserves
        assert!(
            node.get_node_info().channels_info.inbound_capacity_msat
                < CHANNEL_SIZE - REBALANCE_AMOUNT
        ); // smaller instead of equal because of channel reserves

        let invoice = nigiri::issue_invoice(LspdLnd, "test", PAYMENT_AMOUNT, 3600).unwrap();

        let initial_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        node.pay_invoice(invoice).unwrap();
        sleep(Duration::from_secs(2));

        let final_balance = nigiri::query_node_balance(LspdLnd).unwrap();

        assert_eq!(final_balance - initial_balance, PAYMENT_AMOUNT);
    }

    fn assert_invoice_details(
        invoice_details: InvoiceDetails,
        amount_msat: u64,
        description: &str,
        expiry_time: Duration,
        payee_pub_key: &str,
    ) {
        assert_eq!(invoice_details.amount_msat.unwrap(), amount_msat);
        assert_eq!(invoice_details.description, description);
        assert_eq!(invoice_details.expiry_interval, expiry_time);
        assert_eq!(invoice_details.payee_pub_key, payee_pub_key);
    }
}
