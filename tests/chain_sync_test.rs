mod setup;

// Caution: Run these tests sequentially, otherwise they will corrupt each other,
// because they are manipulating their environment:
// cargo test --features nigiri -- --test-threads 1
#[cfg(feature = "nigiri")]
mod chain_sync_test {
    use super::*;
    use bitcoin::hashes::hex::ToHex;
    use std::thread::sleep;
    use std::time::Duration;

    use uniffi_lipalightninglib::config::NodeAddress;

    fn fund_lnd_node(amount_btc: &str) -> Result<(), String> {
        let output = setup::nigiri::exec(vec!["nigiri", "faucet", "lnd", amount_btc]);
        if !output.status.success() {
            return Err(format!("Command `faucet lnd {}` failed", amount_btc));
        }
        Ok(())
    }

    fn lnd_open_channel(node_id: &str) -> Result<(), String> {
        let output = setup::nigiri::exec(vec![
            "nigiri",
            "lnd",
            "openchannel",
            "--private",
            node_id,
            "1000000",
        ]);
        if !output.status.success() {
            return Err(format!(
                "Command `lnd openchannel --private {} 1000000` failed",
                node_id
            ));
        }
        let _json: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| "Invalid json")?;

        Ok(())
    }

    fn mine_blocks(block_amount: &str) -> Result<(), String> {
        let output = setup::nigiri::exec(vec!["nigiri", "rpc", "-generate", block_amount]);
        if !output.status.success() {
            return Err(format!("Command `rpc -generate {}` failed", block_amount));
        }
        Ok(())
    }

    #[test]
    fn test_channel_is_confirmed_chain() {
        setup::nigiri::start();
        let lsp_info = setup::nigiri::query_lnd_info().unwrap();
        let lsp_node = NodeAddress {
            pub_key: lsp_info.pub_key,
            address: "127.0.0.1:9735".to_string(),
        };

        let node = setup::setup(lsp_node).unwrap();
        assert_eq!(node.get_node_info().num_peers, 1);

        let node_id = node.get_node_info().node_pubkey.to_hex();

        for i in 0..11 {
            match fund_lnd_node("0.5") {
                Ok(_) => break,
                Err(_) => {
                    if i < 10 {
                        continue;
                    }
                }
            }
            fund_lnd_node("0.5").unwrap();
        }

        lnd_open_channel(&node_id).unwrap();

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 0);

        mine_blocks("6").unwrap();

        sleep(Duration::from_secs(10));

        assert_eq!(node.get_node_info().num_channels, 1);
        assert_eq!(node.get_node_info().num_usable_channels, 1);
    }
}
