use crate::data_store::DataStore;
use crate::errors::{PayErrorCode, Result};
use crate::interfaces;
use crate::payment::PaymentState;
use crate::task_manager::TaskManager;
use crate::types::ChannelManager;

use bitcoin::hashes::hex::ToHex;
use bitcoin::hashes::{sha256, Hash};
use lightning::events::{Event, EventHandler, PaymentFailureReason, PaymentPurpose};
use log::{error, info, trace};
use std::sync::{Arc, Mutex};

pub(crate) struct LipaEventHandler {
    channel_manager: Arc<ChannelManager>,
    task_manager: Arc<Mutex<TaskManager>>,
    user_event_handler: Box<dyn interfaces::EventHandler>,
    data_store: Arc<Mutex<DataStore>>,
}

impl LipaEventHandler {
    pub fn new(
        channel_manager: Arc<ChannelManager>,
        task_manager: Arc<Mutex<TaskManager>>,
        user_event_handler: Box<dyn interfaces::EventHandler>,
        data_store: Arc<Mutex<DataStore>>,
    ) -> Result<Self> {
        Ok(Self {
            channel_manager,
            task_manager,
            user_event_handler,
            data_store,
        })
    }
}

impl EventHandler for LipaEventHandler {
    fn handle_event(&self, event: Event) {
        trace!("Event occured: {event:?}");

        match event {
            Event::FundingGenerationReady { .. } => {}
            Event::PaymentClaimable {
                receiver_node_id: _,
                payment_hash,
                amount_msat,
                purpose,
                ..
            } => {
                // Note: LDK will not stop an inbound payment from being paid multiple times,
                //       so multiple PaymentReceived events may be generated for the same payment.
                let payment_hash_hex = payment_hash.0.to_hex();
                info!("EVENT: PaymentClaimable - hash: {payment_hash_hex}, amount msat: {amount_msat}");

                let data_store = self.data_store.lock().unwrap();
                if let Ok(payment) = data_store.get_payment(&payment_hash_hex) {
                    if payment.payment_state == PaymentState::Succeeded {
                        info!("Rejecting incoming payment for {amount_msat} msat with hash {payment_hash_hex}, \
 							   because we've already claimed a payment with the same hash");
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                        return;
                    } else if payment.payment_state == PaymentState::InvoiceExpired {
                        info!("Rejecting incoming payment for {amount_msat} msat with hash {payment_hash_hex}, \
							   because the corresponding invoice has expired");
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                        return;
                    }
                }

                match purpose {
                    PaymentPurpose::InvoicePayment {
                        payment_preimage: Some(payment_preimage),
                        ..
                    } => {
                        info!("Claiming incoming invoice payment for {amount_msat} msat with hash {payment_hash_hex}");
                        if data_store
                            .fill_preimage(&payment_hash_hex, &payment_preimage.0.to_hex())
                            .is_err()
                        {
                            error!("Failed to fill preimage in the payment db for payment hash {payment_hash_hex}");
                        }
                        self.channel_manager.claim_funds(payment_preimage);
                    }
                    PaymentPurpose::InvoicePayment {
                        payment_preimage: None,
                        ..
                    } => {
                        error!("Rejecting incoming invoice payment for {amount_msat} msat with hash {payment_hash_hex}, \
								because no preimage was found");
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                    }
                    PaymentPurpose::SpontaneousPayment(payment_preimage) => {
                        info!("Claiming incoming spontaneous payment for {amount_msat} msat with hash {payment_hash_hex}");
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
                let payment_hash = payment_hash.0.to_hex();
                info!("EVENT: PaymentClaimed - hash: {payment_hash} - amount msat: {amount_msat}");
                match purpose {
                    PaymentPurpose::InvoicePayment { .. } => {
                        info!("Claimed incoming invoice payment for {amount_msat} msat with hash {payment_hash}");
                        if self
                            .data_store
                            .lock()
                            .unwrap()
                            .incoming_payment_succeeded(&payment_hash)
                            .is_err()
                        {
                            error!("Failed to persist in the payment db that the receiving payment with hash {payment_hash} has succeeded");
                        }
                        self.user_event_handler.payment_received(payment_hash);
                    }
                    PaymentPurpose::SpontaneousPayment(_) => {
                        info!("Claimed incoming spontaneous payment for {amount_msat} msat with hash {payment_hash}");
                        // TODO: inform consumer of this library about a claimed spontaneous payment
                    }
                }
            }
            Event::PaymentSent {
                payment_id: _,
                payment_preimage,
                payment_hash,
                fee_paid_msat,
            } => {
                let payment_preimage = payment_preimage.0.to_hex();
                let payment_hash = payment_hash.0.to_hex();
                let fee_paid_msat = fee_paid_msat.unwrap_or(0);
                info!("EVENT: PaymentSent - preimage: {payment_preimage}, hash: {payment_hash}, fee: {fee_paid_msat}");
                if self
                    .data_store
                    .lock()
                    .unwrap()
                    .outgoing_payment_succeeded(&payment_hash, &payment_preimage, fee_paid_msat)
                    .is_err()
                {
                    error!("Failed to persist in the payment db that sending payment with hash {payment_hash} has succeeded");
                }
                self.user_event_handler
                    .payment_sent(payment_hash, payment_preimage);
            }
            Event::PaymentFailed {
                payment_id: _,
                payment_hash,
                reason,
            } => {
                let payment_hash = payment_hash.0.to_hex();
                // `reason` could only be None if we deserialize events from old LDK versions
                let reason = PayErrorCode::from_failure_reason(
                    reason.unwrap_or(PaymentFailureReason::PaymentExpired),
                );
                info!("EVENT: PaymentFailed - hash: {payment_hash} because {reason}");
                if self
                    .data_store
                    .lock()
                    .unwrap()
                    .outgoing_payment_failed(&payment_hash, reason)
                    .is_err()
                {
                    error!("Failed to persist in the payment db that sending payment with hash {payment_hash} has failed");
                }
                self.user_event_handler.payment_failed(payment_hash);
            }
            Event::PaymentPathSuccessful {
                payment_id: _,
                payment_hash,
                path,
            } => {
                let payment_hash = match payment_hash {
                    Some(payment_hash) => match sha256::Hash::from_slice(&payment_hash.0) {
                        Ok(hash) => format!("{hash:?}"),
                        Err(_) => {
                            error!(
                                "Failed to convert payment hash to hex (payment_hash: {payment_hash:?})"
                            );
                            "[invalid payment hash]".to_string()
                        }
                    },
                    None => "[unknown payment hash]".to_string(),
                };

                let hops_short_channel_id = path
                    .hops
                    .iter()
                    .map(|hop| hop.short_channel_id.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");

                let blinded_tail_amount_hops = path.blinded_tail.map_or(0, |tail| tail.hops.len());

                info!("Payment with hash {payment_hash} was successfully routed through the following path (Short channel IDs): {hops_short_channel_id} (amount of hops within blinded tail: {blinded_tail_amount_hops})");
            }
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
            Event::SpendableOutputs { outputs } => {
                info!(
                    "EVENT: SpendableOutputs - {} outputs provided",
                    outputs.len()
                );
                let data_store = self.data_store.lock().unwrap();
                for output in outputs {
                    if let Err(e) = data_store.persist_spendable_output(&output) {
                        error!("Failed to persist spendable output in local db - {e}");
                    }
                }
            }
            Event::PaymentForwarded { .. } => {}
            Event::ChannelClosed {
                channel_id,
                user_channel_id: _,
                reason,
            } => {
                let channel_id = channel_id.to_hex();
                info!("EVENT: ChannelClosed - channel {channel_id} closed due to {reason}");
                self.user_event_handler
                    .channel_closed(channel_id, reason.to_string());
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
                let result = if channel_type.supports_zero_conf() {
                    if let Some(lsp_info) = self.task_manager.lock().unwrap().get_lsp_info() {
                        if lsp_info.node_info.pubkey == counterparty_node_id {
                            self.channel_manager
                                .accept_inbound_channel_from_trusted_peer_0conf(
                                    &temporary_channel_id,
                                    &counterparty_node_id,
                                    0u128,
                                )
                        } else if channel_type.requires_zero_conf() {
                            error!(
                                "Unexpected OpenChannelRequest event. \
				 We don't know the peer and it is trying to open a zero-conf channel. \
				 How did this p2p connection get established?"
                            );
                            self.channel_manager.force_close_without_broadcasting_txn(
                                &temporary_channel_id,
                                &counterparty_node_id,
                            )
                        } else {
                            self.channel_manager.accept_inbound_channel(
                                &temporary_channel_id,
                                &counterparty_node_id,
                                0u128,
                            )
                        }
                    } else if channel_type.requires_zero_conf() {
                        error!(
                            "Got OpenChannelRequest event requiring zero-conf, \
			     but we could not connect to LSP to learn if we can trust the remote node"
                        );
                        self.channel_manager.force_close_without_broadcasting_txn(
                            &temporary_channel_id,
                            &counterparty_node_id,
                        )
                    } else {
                        self.channel_manager.accept_inbound_channel(
                            &temporary_channel_id,
                            &counterparty_node_id,
                            0u128,
                        )
                    }
                } else {
                    self.channel_manager.accept_inbound_channel(
                        &temporary_channel_id,
                        &counterparty_node_id,
                        0u128,
                    )
                };
                if let Err(e) = result {
                    error!("Error on handling new OpenChannelRequest: {e:?}");
                }
            }
            Event::HTLCHandlingFailed {
                prev_channel_id,
                failed_next_destination,
            } => {
                let prev_channel_id = prev_channel_id.to_hex();
                info!("EVENT: HTLCHandlingFailed - prev_channel_id: {prev_channel_id} - failed_next_destination: {failed_next_destination:?}");
            }
            Event::HTLCIntercepted { .. } => {
                info!("EVENT: HTLCIntercepted");
            }
            Event::ChannelPending { .. } => {
                info!("EVENT: ChannelPending");
            }
            Event::ChannelReady { .. } => {
                info!("EVENT: ChannelReady");
            }
            Event::BumpTransaction(..) => {
                info!("EVENT: BumpTransaction");
            }
        }
    }
}
