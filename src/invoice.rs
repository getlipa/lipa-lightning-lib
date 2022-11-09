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

    let capacity = calculate_capacity(&channel_manager.list_usable_channels());
    let needs_channel_opening = capacity.inbound_msat < amount_msat;
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
            if let (Some(channel_config), Some(short_channel_id)) =
                (channel.config, channel.get_inbound_payment_scid())
            {
                let fees = RoutingFees {
                    base_msat: channel_config.forwarding_fee_base_msat,
                    proportional_millionths: channel_config.forwarding_fee_proportional_millionths,
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
    route_hints
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Capacity {
    pub inbound_msat: u64,
    pub outbound_msat: u64,
}

/// Returns total inbound/outbound capacity the node can actually receive/send.
/// It excludes non usable channels, pending htlcs, channels reserves, etc.
pub(crate) fn calculate_capacity(channels: &[ChannelDetails]) -> Capacity {
    let (inbound_msat, outbound_msat) = channels
        .iter()
        .filter(|channel| channel.is_usable)
        .map(|channel| {
            (
                channel.inbound_capacity_msat,
                channel.outbound_capacity_msat,
            )
        })
        .fold((0u64, 0u64), |(in1, out1), (in2, out2)| {
            (in1 + in2, out1 + out2)
        });
    Capacity {
        inbound_msat,
        outbound_msat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning::ln::channelmanager::ChannelCounterparty;
    use lightning::ln::features::InitFeatures;
    use lightning::util::config::ChannelConfig;
    use secp256k1::{PublicKey, Secp256k1, ONE_KEY};

    fn channel() -> ChannelDetails {
        let secp = Secp256k1::new();
        let node_id = PublicKey::from_secret_key(&secp, &ONE_KEY);
        let counterparty = ChannelCounterparty {
            node_id,
            features: InitFeatures::empty(),
            unspendable_punishment_reserve: 0u64,
            forwarding_info: None,
            outbound_htlc_minimum_msat: None,
            outbound_htlc_maximum_msat: None,
        };
        let config = ChannelConfig {
            forwarding_fee_proportional_millionths: 0u32,
            forwarding_fee_base_msat: 0u32,
            cltv_expiry_delta: 0u16,
            max_dust_htlc_exposure_msat: 0u64,
            force_close_avoidance_max_fee_satoshis: 0u64,
        };
        ChannelDetails {
            channel_id: [0u8; 32],
            counterparty,
            funding_txo: None,
            channel_type: None,
            short_channel_id: Some(0u64),
            outbound_scid_alias: None,
            inbound_scid_alias: None,
            channel_value_satoshis: 0u64,
            unspendable_punishment_reserve: None,
            user_channel_id: 0u64,
            balance_msat: 0u64,
            outbound_capacity_msat: 0u64,
            next_outbound_htlc_limit_msat: 0u64,
            inbound_capacity_msat: 0u64,
            confirmations_required: None,
            force_close_spend_delay: None,
            is_outbound: false,
            is_channel_ready: false,
            is_usable: false,
            is_public: false,
            inbound_htlc_minimum_msat: None,
            inbound_htlc_maximum_msat: None,
            config: Some(config),
        }
    }

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

    #[test]
    fn test_calculate_capacity() {
        assert_eq!(
            calculate_capacity(&Vec::new()),
            Capacity {
                inbound_msat: 0u64,
                outbound_msat: 0u64,
            }
        );

        let mut channel1 = channel();
        channel1.is_usable = true;
        channel1.inbound_capacity_msat = 1_111;
        channel1.outbound_capacity_msat = 1_222;
        assert_eq!(
            calculate_capacity(&vec![channel1.clone()]),
            Capacity {
                inbound_msat: 1_111u64,
                outbound_msat: 1_222u64,
            }
        );

        let mut channel2 = channel();
        channel2.is_usable = true;
        channel2.inbound_capacity_msat = 90_000;
        channel2.outbound_capacity_msat = 90_111;
        assert_eq!(
            calculate_capacity(&vec![channel2.clone()]),
            Capacity {
                inbound_msat: 90_000u64,
                outbound_msat: 90_111u64,
            }
        );
        assert_eq!(
            calculate_capacity(&vec![channel1.clone(), channel2.clone()]),
            Capacity {
                inbound_msat: 91_111u64,
                outbound_msat: 91_333u64,
            }
        );

        let mut not_usable_channel = channel();
        not_usable_channel.inbound_capacity_msat = 777_777;
        not_usable_channel.outbound_capacity_msat = 888_888;
        assert_eq!(
            calculate_capacity(&vec![not_usable_channel.clone()]),
            Capacity {
                inbound_msat: 0u64,
                outbound_msat: 0u64,
            }
        );
        assert_eq!(
            calculate_capacity(&vec![
                channel1.clone(),
                channel2.clone(),
                not_usable_channel.clone()
            ]),
            Capacity {
                inbound_msat: 91_111u64,
                outbound_msat: 91_333u64,
            }
        );
        assert_eq!(
            calculate_capacity(&vec![channel1.clone(), channel2.clone()]),
            Capacity {
                inbound_msat: 91_111u64,
                outbound_msat: 91_333u64,
            }
        );
    }
}
