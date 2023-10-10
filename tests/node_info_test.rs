mod print_events_handler;
mod setup;

use crate::setup::start_alice;

use bitcoin::secp256k1::PublicKey;
use serial_test::file_serial;
use std::str::FromStr;

#[test]
#[file_serial(key, "/tmp/3l-int-tests-lock")]
fn test_get_node_info() {
    let node = start_alice().unwrap();
    let node_info = node.get_node_info().unwrap();

    assert!(
        PublicKey::from_str(&*node_info.node_pubkey).is_ok(),
        "Node public key is not valid"
    );
    assert!(
        node_info.channels_info.local_balance.sats < 21_000_000 * 100_000_000,
        "Node must not hold more than 21 million BTC on lightning"
    );
    assert!(
        node_info.onchain_balance.sats < 21_000_000 * 100_000_000,
        "Node must not hold more than 21 million BTC on-chain"
    );
}
