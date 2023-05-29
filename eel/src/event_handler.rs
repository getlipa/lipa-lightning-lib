use crate::data_store::DataStore;
use crate::errors::Result;
use crate::interfaces;
use crate::payment::PaymentState;
use crate::task_manager::TaskManager;
use crate::types::ChannelManager;

use crate::fee_estimator::FeeEstimator;
use crate::tx_broadcaster::TxBroadcaster;
use bitcoin::hashes::hex::ToHex;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator as LdkFeeEstimator,
};
use lightning::chain::keysinterface::{KeysManager, SignerProvider, SpendableOutputDescriptor};
use lightning::events::{Event, EventHandler, PaymentPurpose};
use log::{error, info, trace};
use secp256k1::SECP256K1;
use std::sync::{Arc, Mutex};

pub(crate) struct LipaEventHandler {
    channel_manager: Arc<ChannelManager>,
    task_manager: Arc<Mutex<TaskManager>>,
    user_event_handler: Box<dyn interfaces::EventHandler>,
    data_store: Arc<Mutex<DataStore>>,
    keys_manager: Arc<KeysManager>,
    fee_estimator: Arc<FeeEstimator>,
    tx_broadcaster: Arc<TxBroadcaster>,
}

impl LipaEventHandler {
    pub fn new(
        channel_manager: Arc<ChannelManager>,
        task_manager: Arc<Mutex<TaskManager>>,
        user_event_handler: Box<dyn interfaces::EventHandler>,
        data_store: Arc<Mutex<DataStore>>,
        keys_manager: Arc<KeysManager>,
        fee_estimator: Arc<FeeEstimator>,
        tx_broadcaster: Arc<TxBroadcaster>,
    ) -> Result<Self> {
        Ok(Self {
            channel_manager,
            task_manager,
            user_event_handler,
            data_store,
            keys_manager,
            fee_estimator,
            tx_broadcaster,
        })
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
                ..
            } => {
                // Note: LDK will not stop an inbound payment from being paid multiple times,
                //       so multiple PaymentReceived events may be generated for the same payment.
                info!(
                    "EVENT: PaymentClaimable - hash: {} - amount msat: {}",
                    payment_hash.0.to_hex(),
                    amount_msat,
                );

                let data_store = self.data_store.lock().unwrap();

                if let Ok(payment) = data_store.get_payment(&payment_hash.0.to_hex()) {
                    if payment.payment_state == PaymentState::Succeeded {
                        info!("Registered incoming payment for {} msat with hash {}. Rejecting because we've already claimed a payment with the same hash", amount_msat, payment_hash.0.to_hex());
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                        return;
                    } else if payment.payment_state == PaymentState::InvoiceExpired {
                        info!("Registered incoming payment for {} msat with hash {}. Rejecting because the corresponding invoice expired", amount_msat, payment_hash.0.to_hex());
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                        return;
                    }
                }

                match purpose {
                    PaymentPurpose::InvoicePayment {
                        payment_preimage: Some(payment_preimage),
                        ..
                    } => {
                        info!(
                            "Registered incoming invoice payment for {} msat with hash {}",
                            amount_msat,
                            payment_hash.0.to_hex()
                        );
                        if data_store
                            .fill_preimage(
                                &payment_hash.0.as_slice().to_hex(),
                                &payment_preimage.0.as_slice().to_hex(),
                            )
                            .is_err()
                        {
                            error!(
                                "Failed to fill preimage in the payment db for payment hash {}",
                                payment_hash.0.to_hex()
                            );
                        }
                        self.channel_manager.claim_funds(payment_preimage);
                    }
                    PaymentPurpose::InvoicePayment {
                        payment_preimage: None,
                        ..
                    } => {
                        error!(
                            "Registered incoming invoice payment for {} msat with hash {}, but no preimage was found",
                            amount_msat, payment_hash.0.to_hex()
                        );
                        self.channel_manager.fail_htlc_backwards(&payment_hash);
                    }
                    PaymentPurpose::SpontaneousPayment(payment_preimage) => {
                        info!(
                            "Registered incoming spontaneous payment for {} msat with hash {}",
                            amount_msat,
                            payment_hash.0.to_hex()
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
                            "Registered incoming invoice payment for {} msat with hash {}",
                            amount_msat,
                            payment_hash.0.to_hex()
                        );
                        if self
                            .data_store
                            .lock()
                            .unwrap()
                            .incoming_payment_succeeded(&payment_hash.0.as_slice().to_hex())
                            .is_err()
                        {
                            error!("Failed to persist in the payment db that the receiving payment with hash {} has succeeded", payment_hash.0.to_hex());
                        }
                        self.user_event_handler
                            .payment_received(payment_hash.0.to_hex());
                    }
                    PaymentPurpose::SpontaneousPayment(_) => {
                        info!(
                            "Claimed incoming spontaneous payment for {} msat with hash {}",
                            amount_msat,
                            payment_hash.0.to_hex()
                        );
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
                let fee_paid_msat = fee_paid_msat.unwrap_or(0);
                info!(
                    "EVENT: PaymentSent - preimage: {} - hash: {} - fee: {}",
                    payment_preimage.0.to_hex(),
                    payment_hash.0.to_hex(),
                    fee_paid_msat,
                );
                if self
                    .data_store
                    .lock()
                    .unwrap()
                    .outgoing_payment_succeeded(
                        &payment_hash.0.as_slice().to_hex(),
                        &payment_preimage.0.as_slice().to_hex(),
                        fee_paid_msat,
                    )
                    .is_err()
                {
                    error!("Failed to persist in the payment db that sending payment with hash {} has succeeded", payment_hash.0.to_hex());
                }
                self.user_event_handler
                    .payment_sent(payment_hash.0.to_hex(), payment_preimage.0.to_hex());
            }
            Event::PaymentFailed {
                payment_id: _,
                payment_hash,
                reason,
            } => {
                info!(
                    "EVENT: PaymentFailed - hash: {}, {reason:?}",
                    payment_hash.0.to_hex()
                );
                if self
                    .data_store
                    .lock()
                    .unwrap()
                    .new_payment_state(&payment_hash.0.as_slice().to_hex(), PaymentState::Failed)
                    .is_err()
                {
                    error!("Failed to persist in the payment db that sending payment with hash {} has failed", payment_hash.0.to_hex());
                }
                self.user_event_handler
                    .payment_failed(payment_hash.0.to_hex());
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
            Event::SpendableOutputs { outputs } => {
                info!(
                    "EVENT: SpendableOutputs - {} spendable outputs provided",
                    outputs.len()
                );

                let only_non_static_outputs = outputs
                    .iter()
                    .filter(|desc| !matches!(desc, SpendableOutputDescriptor::StaticOutput { .. }))
                    .collect::<Vec<_>>();
                if only_non_static_outputs.is_empty() {
                    return;
                }

                info!(
                    "Creating spending tx only with non static outputs - {} non static output(s) was/were found",
                    only_non_static_outputs.len()
                );
                let destination_script = self.keys_manager.get_destination_script();
                let tx_feerate = self
                    .fee_estimator
                    .get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
                let res = self.keys_manager.spend_spendable_outputs(
                    &only_non_static_outputs,
                    Vec::new(),
                    destination_script,
                    tx_feerate,
                    SECP256K1,
                );
                match res {
                    Ok(spending_tx) => {
                        self.tx_broadcaster.broadcast_transaction(&spending_tx);
                        info!(
                            "Broadcasted non-static output spending tx with id {}",
                            spending_tx.txid()
                        );
                    }
                    Err(err) => {
                        error!("Failed to spend outputs: {err:?}");
                    }
                }
            }
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
                self.user_event_handler
                    .channel_closed(channel_id.to_hex(), reason.to_string());
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
                    error!("Error on handling new OpenChannelRequest: {:?}", e);
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
            Event::ChannelPending { .. } => {
                info!("EVENT: ChannelPending");
            }
            Event::ChannelReady { .. } => {
                info!("EVENT: ChannelReady");
            }
        }
    }
}
