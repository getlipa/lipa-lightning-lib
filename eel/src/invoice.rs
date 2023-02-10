use crate::errors::*;
use crate::lsp::{LspClient, PaymentRequest};
use crate::node_info::get_channels_info;
use crate::types::ChannelManager;
use std::time::{Duration, SystemTime};

use crate::lsp;
use bitcoin::hashes::{sha256, Hash};
use lightning::ln::channelmanager::ChannelDetails;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, InvoiceBuilder, RawInvoice};
use log::info;
use perro::{invalid_input, MapToError, MapToErrorForUnitType, ResultTrait};

pub struct InvoiceDetails {
    pub amount_msat: Option<u64>,
    pub description: String,
    pub payment_hash: String,
    pub payee_pub_key: String,
    pub invoice_timestamp: SystemTime,
    pub expiry_interval: Duration,
}

pub(crate) async fn create_raw_invoice(
    amount_msat: u64,
    currency: Currency,
    description: String,
    channel_manager: &ChannelManager,
    lsp_client: &LspClient,
) -> Result<RawInvoice> {
    // Do we need a new channel to receive this payment?
    let channels_info = get_channels_info(&channel_manager.list_channels());
    let needs_channel_opening = channels_info.inbound_capacity_msat < amount_msat;

    let payee_pubkey = channel_manager.get_our_node_id();

    let (payment_hash, payment_secret, private_routes) = if needs_channel_opening {
        let lsp_info = lsp_client
            .query_info()
            .await
            .lift_invalid_input()
            .prefix_error("Failed to query LSPD")?;

        let lsp_fee = lsp::calculate_fee(amount_msat, &lsp_info.fee);
        if lsp_fee >= amount_msat {
            return Err(invalid_input("Payment amount must be higher than lsp fees"));
        }
        let incoming_amount_msat = amount_msat - lsp_fee;

        info!(
            "Not enough inbound capacity for {} msat, needs channel opening, will only receive {} msat due to LSP fees",
            amount_msat, incoming_amount_msat
        );

        let (payment_hash, payment_secret) = channel_manager
            .create_inbound_payment(Some(incoming_amount_msat), 1000)
            .map_to_invalid_input("Amount is greater than total bitcoin supply")?;

        let payment_request = PaymentRequest {
            payment_hash,
            payment_secret,
            payee_pubkey,
            amount_msat,
        };
        let hint_hop = lsp_client
            .register_payment(&payment_request, &lsp_info)
            .await
            .lift_invalid_input()
            .prefix_error("Failed to register payment")?;
        (
            payment_hash,
            payment_secret,
            vec![RouteHint(vec![hint_hop])],
        )
    } else {
        let (payment_hash, payment_secret) = channel_manager
            .create_inbound_payment(Some(amount_msat), 1000)
            .map_to_invalid_input("Amount is greater than total bitcoin supply")?;

        (
            payment_hash,
            payment_secret,
            construct_private_routes(&channel_manager.list_usable_channels()),
        )
    };

    let payment_hash = sha256::Hash::from_slice(&payment_hash.0)
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
            if let (Some(channel_counterparty_forwarding_info), Some(short_channel_id)) = (
                channel.counterparty.forwarding_info.clone(),
                channel.get_inbound_payment_scid(),
            ) {
                let fees = RoutingFees {
                    base_msat: channel_counterparty_forwarding_info.fee_base_msat,
                    proportional_millionths: channel_counterparty_forwarding_info
                        .fee_proportional_millionths,
                };
                let hint_hop = RouteHintHop {
                    src_node_id: channel.counterparty.node_id,
                    short_channel_id,
                    fees,
                    cltv_expiry_delta: channel_counterparty_forwarding_info.cltv_expiry_delta,
                    htlc_minimum_msat: channel.inbound_htlc_minimum_msat,
                    htlc_maximum_msat: channel.inbound_htlc_maximum_msat,
                };
                route_hints.push(RouteHint(vec![hint_hop]));
            }
        }
    }
    route_hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::channels::channel;

    #[test]
    fn test_construct_private_routes() {
        assert_eq!(construct_private_routes(&Vec::new()), Vec::new());

        let mut channel1 = channel();
        channel1.is_usable = true;
        assert_eq!(construct_private_routes(&vec![channel1.clone()]).len(), 1);

        let mut public_channel = channel();
        public_channel.is_usable = true;
        public_channel.is_public = true;
        assert_eq!(
            construct_private_routes(&vec![public_channel.clone()]).len(),
            0
        );

        let mut channel2 = channel();
        channel2.is_usable = true;
        assert_eq!(
            construct_private_routes(&vec![
                public_channel.clone(),
                channel1.clone(),
                channel2.clone()
            ])
            .len(),
            2
        );
    }
}
