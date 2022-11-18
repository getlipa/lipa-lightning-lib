#![cfg(test)]

use lightning::ln::channelmanager::ChannelDetails;
use lightning::ln::channelmanager::{ChannelCounterparty, CounterpartyForwardingInfo};
use lightning::ln::features::InitFeatures;
use lightning::util::config::ChannelConfig;
use secp256k1::{PublicKey, Secp256k1, ONE_KEY};

pub fn channel() -> ChannelDetails {
    let secp = Secp256k1::new();
    let node_id = PublicKey::from_secret_key(&secp, &ONE_KEY);
    let counterparty = ChannelCounterparty {
        node_id,
        features: InitFeatures::empty(),
        unspendable_punishment_reserve: 0u64,
        forwarding_info: Some(CounterpartyForwardingInfo {
            fee_base_msat: 0u32,
            fee_proportional_millionths: 0u32,
            cltv_expiry_delta: 0u16,
        }),
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
