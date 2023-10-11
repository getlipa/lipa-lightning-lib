mod print_events_handler;
mod setup;

use crate::setup::start_alice;
use perro::Error::InvalidInput;
use std::time::Duration;

use serial_test::file_serial;
use uniffi_lipalightninglib::{OfferInfo, OfferKind, OfferStatus, TopupCurrency};

#[test]
#[file_serial(key, "/tmp/3l-int-tests-lock")]
fn test_topup() {
    let node = start_alice().unwrap();

    node.register_fiat_topup(
        None,
        "CH8689144834469929874".to_string(),
        TopupCurrency::CHF,
    )
    .expect("Couldn't register topup without email");

    node.register_fiat_topup(
        Some("alice-topup@integration.lipa.swiss".to_string()),
        "CH0389144436836555818".to_string(),
        TopupCurrency::CHF,
    )
    .expect("Couldn't register topup with email");

    node.register_fiat_topup(
        Some("alice-topup@integration.lipa.swiss".to_string()),
        "CH9289144414389576442".to_string(),
        TopupCurrency::CHF,
    )
    .expect("Couldn't register second topup with used email");

    node.register_fiat_topup(
        Some("alice-topup2@integration.lipa.swiss".to_string()),
        "CH9289144414389576442".to_string(),
        TopupCurrency::CHF,
    )
    .expect("Couldn't re-register topup with different email");

    let result = node.register_fiat_topup(
        Some("alice-topup@integration.lipa.swiss".to_string()),
        "INVALID_IBAN".to_string(),
        TopupCurrency::CHF,
    );
    assert!(matches!(result, Err(InvalidInput { .. })));

    let mut expected_offer_count = node.query_uncompleted_offers().unwrap().len() + 1;

    // `DK1125112511251125` triggers a new topup ready to be collected
    node.register_fiat_topup(None, "DK1125112511251125".to_string(), TopupCurrency::EUR)
        .unwrap();

    wait_for_condition!(
        node.query_uncompleted_offers().unwrap().len() == expected_offer_count,
        "Offer count didn't change as expected in the given timeframe",
        6 * 5,
        Duration::from_secs(10)
    );
    let mut uncompleted_offers = node.query_uncompleted_offers().unwrap();
    uncompleted_offers.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    assert!(matches!(
        uncompleted_offers.get(0).unwrap(),
        OfferInfo {
            offer_kind: OfferKind::Pocket {
                topup_value_minor_units: 10,
                ..
            },
            status: OfferStatus::READY,
            ..
        }
    ));

    expected_offer_count = uncompleted_offers.len() + 1;

    // `DK2225222522252225` triggers a new topup that is directly refunded
    node.register_fiat_topup(None, "DK2225222522252225".to_string(), TopupCurrency::EUR)
        .unwrap();

    wait_for_condition!(
        node.query_uncompleted_offers().unwrap().len() == expected_offer_count,
        "Offer count didn't change as expected in the given timeframe",
        6 * 5,
        Duration::from_secs(10)
    );
    let mut uncompleted_offers = node.query_uncompleted_offers().unwrap();
    uncompleted_offers.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    assert!(matches!(
        uncompleted_offers.get(0).unwrap(),
        OfferInfo {
            offer_kind: OfferKind::Pocket {
                topup_value_minor_units: 100100,
                ..
            },
            status: OfferStatus::REFUNDED,
            ..
        }
    ));

    let refunded_topup_id = match uncompleted_offers.get(0).unwrap() {
        OfferKind::Pocket { id, .. } => id.to_string(),
    };

    node.hide_topup(refunded_topup_id.clone()).unwrap();

    let uncompleted_offers = node.query_uncompleted_offers().unwrap();
    uncompleted_offers.iter().find(|&&o| match o {
        OfferKind::Pocket { id, .. } => id.to_string(),
    } == refunded_topup_id);
}
