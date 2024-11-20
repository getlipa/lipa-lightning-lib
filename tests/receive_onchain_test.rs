mod print_events_handler;
mod setup;

use crate::setup::start_node;

use serial_test::file_serial;

#[test]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_receive_onchain() {
    let node = start_node().unwrap();

    let swap_info = node.onchain().swap().create(None).unwrap();
    assert!(swap_info.address.starts_with("bc1"));
    assert!(swap_info.min_deposit.sats < swap_info.max_deposit.sats);

    // Calling a second time isn't an issue because no swap has been started
    let swap_info = node.onchain().swap().create(None).unwrap();
    assert!(swap_info.address.starts_with("bc1"));
    assert!(swap_info.min_deposit.sats < swap_info.max_deposit.sats);

    let lsp_fee = node
        .lightning()
        .calculate_lsp_fee_for_amount(100000, None)
        .unwrap();
    let lsp_fee_high_expiry = node
        .lightning()
        .calculate_lsp_fee_for_amount(100000, Some(2 * 7 * 24 * 6 * 6))
        .unwrap();
    assert_ne!(
        lsp_fee.lsp_fee_params.clone().unwrap().valid_until,
        lsp_fee_high_expiry
            .lsp_fee_params
            .clone()
            .unwrap()
            .valid_until
    );
    assert!(lsp_fee.lsp_fee.sats < lsp_fee_high_expiry.lsp_fee.sats);

    let swap_info = node
        .onchain()
        .swap()
        .create(lsp_fee.lsp_fee_params)
        .unwrap();
    assert!(swap_info.address.starts_with("bc1"));
}
