use crate::types::ChannelManager;

use crate::callbacks::EventsCallback;
use bitcoin::hashes::hex::ToHex;
use bitcoin::secp256k1::PublicKey;
use lightning::util::events::{Event, EventHandler, PaymentPurpose};
use log::{error, info, trace};
use std::sync::Arc;

pub(crate) struct LipaEventHandler {
    lsp_pubkey: PublicKey,
    channel_manager: Arc<ChannelManager>,
    events_callback: Box<dyn EventsCallback>,
}

impl LipaEventHandler {
    pub fn new(
        lsp_pubkey: PublicKey,
        channel_manager: Arc<ChannelManager>,
        events_callback: Box<dyn EventsCallback>,
    ) -> Self {
        Self {
            lsp_pubkey,
            channel_manager,
            events_callback,
        }
    }
}

impl EventHandler for LipaEventHandler {
    fn handle_event(&self, event: Event) {
        trace!("Event occured: {:?}", event);

        match event {
            Event::FundingGenerationReady { .. } => {}
            Event::PaymentClaimable {
                receiver_node_id: _,
                payment_hash,
                amount_msat,
                purpose,
                via_channel_id: _,
                via_user_channel_id: _,
            } => {
                // Note: LDK will not stop an inbound payment from being paid multiple times,
                //       so multiple PaymentReceived events may be generated for the same payment.
                // Todo: This needs more research on what exactly is happening under the hood
                //       and what the correct behaviour should be to deal with this situation.

                info!(
                    "EVENT: PaymentClaimable - hash: {} - amount msat: {}",
                    payment_hash.0.to_hex(),
                    amount_msat,
                );

                match purpose {
                    PaymentPurpose::InvoicePayment {
                        payment_preimage: Some(payment_preimage),
                        ..
                    } => {
                        info!(
                            "Registered incoming invoice payment for {} msat with hash {:?}",
                            amount_msat, payment_hash
                        );
                        self.channel_manager.claim_funds(payment_preimage);
                    }
                    PaymentPurpose::InvoicePayment {
                        payment_preimage: None,
                        ..
                    } => {
                        error!(
                            "Registered incoming invoice payment for {} msat with hash {:?}, but no preimage was found",
                            amount_msat, payment_hash
                        );
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                    }
                    PaymentPurpose::SpontaneousPayment(payment_preimage) => {
                        info!(
                            "Registered incoming spontaneous payment for {} msat with hash {:?}",
                            amount_msat, payment_hash
                        );
                        self.channel_manager.claim_funds(payment_preimage);
                    }
                }
            }
            Event::PaymentClaimed {
                receiver_node_id: _,
                payment_hash,
                amount_msat,
                purpose,
            } => {
                info!(
                    "EVENT: PaymentClaimed - hash: {} - amount msat: {}",
                    payment_hash.0.to_hex(),
                    amount_msat,
                );
                match purpose {
                    PaymentPurpose::InvoicePayment { .. } => {
                        info!(
                            "Registered incoming invoice payment for {} msat with hash {:?}",
                            amount_msat, payment_hash
                        );
                        // TODO: Handle unwrap()
                        self.events_callback
                            .payment_received(payment_hash.0.to_hex(), amount_msat)
                            .unwrap();
                    }
                    PaymentPurpose::SpontaneousPayment(_) => {
                        info!(
                            "Claimed incoming spontaneous payment for {} msat with hash {:?}",
                            amount_msat, payment_hash
                        );
                        // TODO: inform consumer of this library about a claimed spontaneous payment
                        //      We can leave this for later as spontaneous payments are not a
                        //      feature of the MVP.
                    }
                }
            }
            Event::PaymentSent {
                payment_id: _,
                payment_preimage,
                payment_hash,
                fee_paid_msat,
            } => {
                let fee_paid_msat = fee_paid_msat.unwrap_or(0);
                info!(
                    "EVENT: PaymentSent - preimage: {} - hash: {} - fee: {}",
                    payment_preimage.0.to_hex(),
                    payment_hash.0.to_hex(),
                    fee_paid_msat,
                );
                // TODO: Handle unwrap()
                self.events_callback
                    .payment_sent(
                        payment_preimage.0.to_hex(),
                        payment_hash.0.to_hex(),
                        fee_paid_msat,
                    )
                    .unwrap();
            }
            Event::PaymentFailed {
                payment_id: _,
                payment_hash,
            } => {
                info!(
                    "EVENT: PaymentFailed - preimage: {}",
                    payment_hash.0.to_hex()
                );
                // TODO: Handle unwrap()
                self.events_callback
                    .payment_failed(payment_hash.0.to_hex())
                    .unwrap();
            }
            Event::PaymentPathSuccessful { .. } => {}
            Event::PaymentPathFailed { .. } => {}
            Event::ProbeSuccessful { .. } => {}
            Event::ProbeFailed { .. } => {}
            Event::PendingHTLCsForwardable {
                time_forwardable: _,
            } => {
                // CAUTION:
                // The name of this event "PendingHTLCsForwardable" is a potentially misleading.
                // It is not only triggered when the node received HTLCs to forward to another node,
                // but also when the node receives an HTLC for itself (incoming payment).

                // The variable time_forwardable is meant to be used to obfuscate the timing
                // of when a payment is being forwarded/accepted.
                // For the time being (while Lipa is the only LSP for these wallets)
                // this measure may only obfuscate for routing nodes on the payment path,
                // whether a payment that is being routed through the Lipa-LSP
                // is being routed towards a Lipa user or towards a third party node.
                // Since the Lipa users don't forward payments anyways,
                // the Lipa-LSP itself will always know the destination of the payment anyways.
                // For this reason, we don't deem this timely obfuscation to provide
                // any meaningful privacy advantage for our case and therefore
                // do not delay the acceptance of HTLC using the time_forwardable variable for now.

                info!("EVENT: PendingHTLCsForwardable");

                self.channel_manager.process_pending_htlc_forwards();
            }
            Event::SpendableOutputs { .. } => {}
            Event::PaymentForwarded { .. } => {}
            Event::ChannelClosed {
                channel_id,
                user_channel_id: _,
                reason,
            } => {
                info!(
                    "EVENT: ChannelClosed - channel {} closed due to {}",
                    channel_id.to_hex(),
                    reason
                );
                // TODO: Handle unwrap()
                self.events_callback
                    .channel_closed(channel_id.to_hex(), reason.to_string())
                    .unwrap();
            }
            Event::DiscardFunding { .. } => {}
            Event::OpenChannelRequest {
                temporary_channel_id,
                counterparty_node_id,
                funding_satoshis: _,
                push_msat: _,
                channel_type,
            } => {
                info!("EVENT: OpenChannelRequest");
                if counterparty_node_id == self.lsp_pubkey && channel_type.supports_zero_conf() {
                    self.channel_manager
                        .accept_inbound_channel_from_trusted_peer_0conf(
                            &temporary_channel_id,
                            &counterparty_node_id,
                            0u128,
                        )
                        .unwrap();
                } else if channel_type.requires_zero_conf() {
                    error!("Unexpected OpenChannelRequest event. We don't know the peer and it is trying to open a zero-conf channel. How did this p2p connection get established?");
                } else {
                    self.channel_manager
                        .accept_inbound_channel(&temporary_channel_id, &counterparty_node_id, 0u128)
                        .unwrap();
                }
            }
            Event::HTLCHandlingFailed {
                prev_channel_id,
                failed_next_destination,
            } => {
                info!(
                    "EVENT: HTLCHandlingFailed - prev_channel_id: {} - failed_next_destination: {:?}",
                    prev_channel_id.to_hex(),
                    failed_next_destination
                );
            }
            Event::HTLCIntercepted { .. } => {
                info!("EVENT: HTLCIntercepted");
            }
            Event::ChannelReady { .. } => {
                info!("EVENT: ChannelReady");
            }
        }
    }
}
