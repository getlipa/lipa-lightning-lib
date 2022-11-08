use crate::errors::*;
use crate::lsp::{LspClient, PaymentRequest};
use crate::types::ChannelManager;

use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::hashes::sha256;
use lightning::ln::channelmanager::ChannelDetails;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, InvoiceBuilder, RawInvoice};
use log::info;

pub(crate) fn create_raw_invoice(
    amount_msat: u64,
    currency: Currency,
    description: String,
    channel_manager: &ChannelManager,
    lsp_client: &LspClient,
) -> LipaResult<RawInvoice> {
    let (payment_hash, payment_secret) = channel_manager
        .create_inbound_payment(Some(amount_msat), 1000)
        .map_to_invalid_input("Amount is greater than total bitcoin supply")?;
    let payee_pubkey = channel_manager.get_our_node_id();

    let inbound_capacity_msat = get_inbound_capacity_msat(&channel_manager.list_usable_channels());
    let needs_channel_opening = inbound_capacity_msat < amount_msat;
    let private_routes = if needs_channel_opening {
        info!(
            "Not enough inbound capacity for {} msat, needs channel opening",
            amount_msat
        );
        let payment_request = PaymentRequest {
            payment_hash,
            payment_secret,
            payee_pubkey,
            amount_msat,
        };
        let lsp_info = lsp_client
            .query_info()
            .lift_invalid_input()
            .prefix_error("Failed to query LSPD")?;
        let hint_hop = lsp_client
            .register_payment(&payment_request, &lsp_info)
            .lift_invalid_input()
            .prefix_error("Failed to register payment")?;
        vec![RouteHint(vec![hint_hop])]
    } else {
        construct_private_routes(&channel_manager.list_usable_channels())
    };

    // TODO: Report it to LDK.
    // Ugly conversion from lightning::ln::PaymentHash to bitcoin::hashes::sha256::Hash.
    let payment_hash = sha256::Hash::from_hex(&payment_hash.0.to_hex())
        .map_to_permanent_failure("Failed to convert payment hash")?;
    let mut builder = InvoiceBuilder::new(currency)
        .description(description)
        .payment_hash(payment_hash)
        .payment_secret(payment_secret)
        .payee_pub_key(payee_pubkey)
        .amount_milli_satoshis(amount_msat)
        .current_timestamp()
        .min_final_cltv_expiry(144);
    for private_route in private_routes {
        builder = builder.private_route(private_route);
    }

    builder
        .build_raw()
        .map_to_permanent_failure("Failed to construct invoice")
}

fn construct_private_routes(channels: &Vec<ChannelDetails>) -> Vec<RouteHint> {
    let mut route_hints = Vec::new();
    for channel in channels {
        if channel.is_usable && !channel.is_public {
            if let Some(channel_config) = channel.config {
                if let Some(short_channel_id) = channel.get_inbound_payment_scid() {
                    let fees = RoutingFees {
                        base_msat: channel_config.forwarding_fee_base_msat,
                        proportional_millionths: channel_config
                            .forwarding_fee_proportional_millionths,
                    };
                    let hint_hop = RouteHintHop {
                        src_node_id: channel.counterparty.node_id,
                        short_channel_id,
                        fees,
                        cltv_expiry_delta: channel_config.cltv_expiry_delta,
                        htlc_minimum_msat: channel.inbound_htlc_minimum_msat,
                        htlc_maximum_msat: channel.inbound_htlc_maximum_msat,
                    };
                    route_hints.push(RouteHint(vec![hint_hop]));
                }
            }
        }
    }
    route_hints
}

fn get_inbound_capacity_msat(channels: &[ChannelDetails]) -> u64 {
    channels
        .iter()
        .filter(|channel| channel.is_usable)
        .map(|channel| channel.inbound_capacity_msat)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construct_private_routes() {
        assert_eq!(construct_private_routes(&Vec::new()), Vec::new());
    }

    #[test]
    fn no_capacity() {
        assert_eq!(get_inbound_capacity_msat(&Vec::new()), 0);
    }
}
