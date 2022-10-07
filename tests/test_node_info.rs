mod setup;

use bitcoin::secp256k1::PublicKey;

#[cfg(test)]
mod node_info_tests {
    use super::*;

    #[test]
    fn test_get_node_info() {
        let node = setup::setup().unwrap();
        let node_info = node.get_node_info();

        assert!(
            PublicKey::from_slice(&*node_info.node_pubkey).is_ok(),
            "Node public key is not valid"
        );
        assert!(
            node_info.num_channels >= node_info.num_usable_channels,
            "Number of channels must be greater or equal to number of usable channels"
        );
        assert!(
            node.get_node_info().local_balance_msat < 21_000_000 * 100_000_000,
            "Node must not hold more than 21 million BTC"
        );
    }
}
