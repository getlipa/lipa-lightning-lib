use crate::types::ChannelManager;

use bitcoin::secp256k1::PublicKey;
use lightning::util::events::{Event, EventHandler};
use std::sync::Arc;

pub(crate) struct LipaEventHandler {
    lsp_pubkey: PublicKey,
    channel_manager: Arc<ChannelManager>,
}

impl LipaEventHandler {
    pub fn new(lsp_pubkey: PublicKey, channel_manager: Arc<ChannelManager>) -> Self {
        Self {
            lsp_pubkey,
            channel_manager,
        }
    }
}

impl EventHandler for LipaEventHandler {
    fn handle_event(&self, event: &Event) {
        match event {
            Event::FundingGenerationReady { .. } => {}
            Event::PaymentReceived { .. } => {}
            Event::PaymentClaimed { .. } => {}
            Event::PaymentSent { .. } => {}
            Event::PaymentFailed { .. } => {}
            Event::PaymentPathSuccessful { .. } => {}
            Event::PaymentPathFailed { .. } => {}
            Event::ProbeSuccessful { .. } => {}
            Event::ProbeFailed { .. } => {}
            Event::PendingHTLCsForwardable { .. } => {}
            Event::SpendableOutputs { .. } => {}
            Event::PaymentForwarded { .. } => {}
            Event::ChannelClosed { .. } => {}
            Event::DiscardFunding { .. } => {}
            Event::OpenChannelRequest {
                temporary_channel_id,
                counterparty_node_id,
                funding_satoshis: _,
                push_msat: _,
                channel_type,
            } => {
                if counterparty_node_id == &self.lsp_pubkey && channel_type.supports_zero_conf() {
                    self.channel_manager
                        .accept_inbound_channel_from_trusted_peer_0conf(
                            temporary_channel_id,
                            counterparty_node_id,
                            0u64,
                        )
                        .unwrap();
                } else {
                    self.channel_manager
                        .accept_inbound_channel(temporary_channel_id, counterparty_node_id, 0u64)
                        .unwrap();
                }
            }
            Event::HTLCHandlingFailed { .. } => {}
        }
    }
}
