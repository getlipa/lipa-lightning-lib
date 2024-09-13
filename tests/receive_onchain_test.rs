mod print_events_handler;
mod setup;

use crate::setup::start_node;

use serial_test::file_serial;

#[test]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_receive_onchain() {
    let node = start_node().unwrap();

    let swap_info = node.generate_swap_address(None).unwrap();
    assert!(swap_info.address.starts_with("bc1"));
    assert!(swap_info.min_deposit.sats < swap_info.max_deposit.sats);

    // Calling a second time isn't an issue because no swap has been started
    let swap_info = node.generate_swap_address(None).unwrap();
    assert!(swap_info.address.starts_with("bc1"));
    assert!(swap_info.min_deposit.sats < swap_info.max_deposit.sats);

    let lsp_fee_params = node.calculate_lsp_fee(100000).unwrap().lsp_fee_params;
    let swap_info = node.generate_swap_address(lsp_fee_params).unwrap();
    assert!(swap_info.address.starts_with("bc1"));
}
