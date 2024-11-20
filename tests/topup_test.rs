mod print_events_handler;
mod setup;

use crate::setup::start_node;
use perro::Error::InvalidInput;
use std::time::Duration;

use serial_test::file_serial;
use uniffi_lipalightninglib::{ActionRequiredItem, OfferInfo, OfferKind, OfferStatus};

#[test]
#[file_serial(key, path => "/tmp/3l-int-tests-lock")]
fn test_topup() {
    let node = start_node().unwrap();

    node.fiat_topup()
        .register(None, "CH8689144834469929874".to_string(), "CHF".to_string())
        .expect("Couldn't register topup without email");

    node.fiat_topup()
        .register(
            Some("alice-topup@integration.lipa.swiss".to_string()),
            "CH0389144436836555818".to_string(),
            "chf".to_string(),
        )
        .expect("Couldn't register topup with email");

    node.fiat_topup()
        .register(
            Some("alice-topup@integration.lipa.swiss".to_string()),
            "CH9289144414389576442".to_string(),
            "chf".to_string(),
        )
        .expect("Couldn't register second topup with used email");

    node.fiat_topup()
        .register(
            Some("alice-topup2@integration.lipa.swiss".to_string()),
            "CH9289144414389576442".to_string(),
            "chf".to_string(),
        )
        .expect("Couldn't re-register topup with different email");

    let result = node.fiat_topup().register(
        Some("alice-topup@integration.lipa.swiss".to_string()),
        "INVALID_IBAN".to_string(),
        "chf".to_string(),
    );
    assert!(matches!(result, Err(InvalidInput { .. })));

    let action_required_items = node.actions_required().list().unwrap();

    let expected_offer_count =
        offer_info_from_actions_required_list(&action_required_items).len() + 1;

    // `DK1125112511251125` triggers a new topup ready to be collected
    node.fiat_topup()
        .register(None, "DK1125112511251125".to_string(), "eur".to_string())
        .unwrap();

    wait_for_condition!(
        offer_info_from_actions_required_list(&node.actions_required().list().unwrap()).len()
            == expected_offer_count,
        "Offer count didn't change as expected in the given timeframe",
        6 * 5,
        Duration::from_secs(10)
    );
    let mut uncompleted_offers =
        offer_info_from_actions_required_list(&node.actions_required().list().unwrap());
    uncompleted_offers.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    assert!(matches!(
        uncompleted_offers.first().unwrap(),
        OfferInfo {
            offer_kind: OfferKind::Pocket { .. },
            status: OfferStatus::READY,
            ..
        }
    ));

    let expected_offer_count = uncompleted_offers.len() + 1;

    // `DK2225222522252225` triggers a new topup that is directly refunded
    // The email achieves the same for the mocked pocket client
    node.fiat_topup()
        .register(
            Some("refund@top.up".to_string()),
            "DK2225222522252225".to_string(),
            "eur".to_string(),
        )
        .unwrap();

    wait_for_condition!(
        offer_info_from_actions_required_list(&node.actions_required().list().unwrap()).len()
            == expected_offer_count,
        "Offer count didn't change as expected in the given timeframe",
        6 * 5,
        Duration::from_secs(10)
    );
    let mut uncompleted_offers =
        offer_info_from_actions_required_list(&node.actions_required().list().unwrap());
    uncompleted_offers.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    assert!(matches!(
        uncompleted_offers.first().unwrap(),
        OfferInfo {
            offer_kind: OfferKind::Pocket { .. },
            status: OfferStatus::REFUNDED,
            ..
        }
    ));

    let refunded_topup_id = match &uncompleted_offers.first().unwrap().offer_kind {
        OfferKind::Pocket { id, .. } => id.to_string(),
    };

    node.actions_required()
        .dismiss_topup(refunded_topup_id.clone())
        .unwrap();

    let uncompleted_offers =
        offer_info_from_actions_required_list(&node.actions_required().list().unwrap());
    uncompleted_offers.iter().find(|o| match &o.offer_kind {
        OfferKind::Pocket { id, .. } => id.to_string(),
    } == refunded_topup_id);
}

fn offer_info_from_actions_required_list(list: &[ActionRequiredItem]) -> Vec<OfferInfo> {
    list.iter()
        .filter_map(|i| match i {
            ActionRequiredItem::UncompletedOffer { offer } => Some(offer.clone()),
            ActionRequiredItem::UnresolvedFailedSwap { .. } => None,
            ActionRequiredItem::ChannelClosesFundsAvailable { .. } => None,
        })
        .collect::<Vec<_>>()
}
