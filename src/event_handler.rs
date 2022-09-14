use crate::{
    hex_utils, ChannelManager, HTLCStatus, LightningLogger, MillisatAmount, PaymentInfo,
    PaymentInfoStorage,
};
use bitcoin::Network;
use bitcoin_bech32::WitnessProgram;
use lightning::routing::gossip;
use lightning::routing::gossip::NodeId;
use lightning::util::events::{Event, EventHandler, PaymentPurpose};
use log::info;
use rand::{thread_rng, Rng};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;

pub(crate) type NetworkGraph = gossip::NetworkGraph<Arc<LightningLogger>>;

pub(crate) struct LipaEventHandler {
    pub(crate) channel_manager: Arc<ChannelManager>,
    // pub(crate) electrum_client: Arc<EsploraClient>,
    pub(crate) network_graph: Arc<NetworkGraph>,
    // pub(crate) keys_manager: Arc<KeysManager>,
    pub(crate) inbound_payments: PaymentInfoStorage,
    pub(crate) outbound_payments: PaymentInfoStorage,
    pub(crate) network: Network,
    pub(crate) tokio_handle: Handle,
}
/*
impl LipaEventHandler {
    pub(crate) fn new(
        channel_manager: Arc<ChannelManager>,
        bitcoind_client: Arc<BitcoindClient>,
        network_graph: Arc<NetworkGraph>,
        keys_manager: Arc<KeysManager>,
        inbound_payments: PaymentInfoStorage,
        outbound_payments: PaymentInfoStorage,
        network: Network,
        tokio_handle: Handle
    ) -> Self {
        LipaEventHandler{
            channel_manager,
            bitcoind_client,
            network_graph,
            keys_manager,
            inbound_payments,
            outbound_payments,
            network,
            tokio_handle,
        }
    }
}*/

impl EventHandler for LipaEventHandler {
    fn handle_event(&self, event: &Event) {
        match event {
            Event::FundingGenerationReady {
                temporary_channel_id: _,
                counterparty_node_id: _,
                channel_value_satoshis,
                output_script,
                ..
            } => {
                // Construct the raw transaction with one output, that is paid the amount of the
                // channel.
                let addr = WitnessProgram::from_scriptpubkey(
                    &output_script[..],
                    match self.network {
                        Network::Bitcoin => bitcoin_bech32::constants::Network::Bitcoin,
                        Network::Testnet => bitcoin_bech32::constants::Network::Testnet,
                        Network::Regtest => bitcoin_bech32::constants::Network::Regtest,
                        Network::Signet => bitcoin_bech32::constants::Network::Signet,
                    },
                )
                .expect("Lightning funding tx should always be to a SegWit output")
                .to_address();
                let mut outputs = vec![HashMap::with_capacity(1)];
                outputs[0].insert(addr, *channel_value_satoshis as f64 / 100_000_000.0);

                /* todo implement


                               let raw_tx = self
                                    .tokio_handle
                                    .block_on(self.bitcoind_client.create_raw_transaction(outputs));

                               // Have your wallet put the inputs into the transaction such that the output is
                               // satisfied.
                               let funded_tx = self
                                   .tokio_handle
                                   .block_on(self.bitcoind_client.fund_raw_transaction(raw_tx));

                               // Sign the final funding transaction and broadcast it.
                               let signed_tx = self.tokio_handle.block_on(
                                   self.bitcoind_client
                                       .sign_raw_transaction_with_wallet(funded_tx.hex),
                               );
                               assert!(signed_tx.complete);
                               let final_tx: Transaction =
                                   encode::deserialize(&hex_utils::to_vec(&signed_tx.hex).unwrap()).unwrap();
                               // Give the funding transaction back to LDK for opening the channel.
                               if self
                                   .channel_manager
                                   .funding_transaction_generated(
                                       temporary_channel_id,
                                       counterparty_node_id,
                                       final_tx,
                                   )
                                   .is_err()
                               {
                                   error!(
                                   "\nChannel went away before we could fund it. The peer disconnected or refused the channel.");
                               }

                */
            }
            Event::PaymentReceived {
                payment_hash,
                purpose,
                amount_msat,
            } => {
                info!(
                    "\nEVENT: received payment from payment hash {} of {} millisatoshis",
                    hex_utils::hex_str(&payment_hash.0),
                    amount_msat,
                );
                let payment_preimage = match purpose {
                    PaymentPurpose::InvoicePayment {
                        payment_preimage, ..
                    } => *payment_preimage,
                    PaymentPurpose::SpontaneousPayment(preimage) => Some(*preimage),
                };
                self.channel_manager.claim_funds(payment_preimage.unwrap());
            }
            Event::PaymentClaimed {
                payment_hash,
                purpose,
                amount_msat,
            } => {
                info!(
                    "\nEVENT: claimed payment from payment hash {} of {} millisatoshis",
                    hex_utils::hex_str(&payment_hash.0),
                    amount_msat,
                );
                let (payment_preimage, payment_secret) = match purpose {
                    PaymentPurpose::InvoicePayment {
                        payment_preimage,
                        payment_secret,
                        ..
                    } => (*payment_preimage, Some(*payment_secret)),
                    PaymentPurpose::SpontaneousPayment(preimage) => (Some(*preimage), None),
                };
                let mut payments = self.inbound_payments.lock().unwrap();
                match payments.entry(*payment_hash) {
                    Entry::Occupied(mut e) => {
                        let payment = e.get_mut();
                        payment.status = HTLCStatus::Succeeded;
                        payment.preimage = payment_preimage;
                        payment.secret = payment_secret;
                    }
                    Entry::Vacant(e) => {
                        e.insert(PaymentInfo {
                            preimage: payment_preimage,
                            secret: payment_secret,
                            status: HTLCStatus::Succeeded,
                            amt_msat: MillisatAmount(Some(*amount_msat)),
                        });
                    }
                }
            }
            Event::PaymentSent {
                payment_preimage,
                payment_hash,
                fee_paid_msat,
                ..
            } => {
                let mut payments = self.outbound_payments.lock().unwrap();
                for (hash, payment) in payments.iter_mut() {
                    if *hash == *payment_hash {
                        payment.preimage = Some(*payment_preimage);
                        payment.status = HTLCStatus::Succeeded;
                        info!(
                            "\nEVENT: successfully sent payment of {} millisatoshis{} from \
								 payment hash {:?} with preimage {:?}",
                            payment.amt_msat,
                            if let Some(fee) = fee_paid_msat {
                                format!(" (fee {} msat)", fee)
                            } else {
                                "".to_string()
                            },
                            hex_utils::hex_str(&payment_hash.0),
                            hex_utils::hex_str(&payment_preimage.0)
                        );
                    }
                }
            }
            Event::OpenChannelRequest { .. } => {
                // Unreachable, we don't set manually_accept_inbound_channels
            }
            Event::PaymentPathSuccessful { .. } => {}
            Event::PaymentPathFailed { .. } => {}
            Event::ProbeSuccessful { .. } => {}
            Event::ProbeFailed { .. } => {}
            Event::PaymentFailed { payment_hash, .. } => {
                info!(
                "\nEVENT: Failed to send payment to payment hash {:?}: exhausted payment retry attempts",
                hex_utils::hex_str(&payment_hash.0)
            );

                let mut payments = self.outbound_payments.lock().unwrap();
                if payments.contains_key(payment_hash) {
                    let payment = payments.get_mut(payment_hash).unwrap();
                    payment.status = HTLCStatus::Failed;
                }
            }
            Event::PaymentForwarded {
                prev_channel_id,
                next_channel_id,
                fee_earned_msat,
                claim_from_onchain_tx,
            } => {
                let read_only_network_graph = self.network_graph.read_only();
                let nodes = read_only_network_graph.nodes();
                let channels = self.channel_manager.list_channels();

                let node_str = |channel_id: &Option<[u8; 32]>| match channel_id {
                    None => String::new(),
                    Some(channel_id) => match channels.iter().find(|c| c.channel_id == *channel_id)
                    {
                        None => String::new(),
                        Some(channel) => {
                            match nodes.get(&NodeId::from_pubkey(&channel.counterparty.node_id)) {
                                None => "private node".to_string(),
                                Some(node) => match &node.announcement_info {
                                    None => "unnamed node".to_string(),
                                    Some(announcement) => {
                                        format!("node {}", announcement.alias)
                                    }
                                },
                            }
                        }
                    },
                };
                let channel_str = |channel_id: &Option<[u8; 32]>| {
                    channel_id
                        .map(|channel_id| {
                            format!(" with channel {}", hex_utils::hex_str(&channel_id))
                        })
                        .unwrap_or_default()
                };
                let from_prev_str = format!(
                    " from {}{}",
                    node_str(prev_channel_id),
                    channel_str(prev_channel_id)
                );
                let to_next_str = format!(
                    " to {}{}",
                    node_str(next_channel_id),
                    channel_str(next_channel_id)
                );

                let from_onchain_str = if *claim_from_onchain_tx {
                    "from onchain downstream claim"
                } else {
                    "from HTLC fulfill message"
                };
                if let Some(fee_earned) = fee_earned_msat {
                    info!(
                        "\nEVENT: Forwarded payment{}{}, earning {} msat {}",
                        from_prev_str, to_next_str, fee_earned, from_onchain_str
                    );
                } else {
                    info!(
                        "\nEVENT: Forwarded payment{}{}, claiming onchain {}",
                        from_prev_str, to_next_str, from_onchain_str
                    );
                }
            }
            Event::HTLCHandlingFailed { .. } => {}
            Event::PendingHTLCsForwardable { time_forwardable } => {
                let forwarding_channel_manager = self.channel_manager.clone();
                let min = time_forwardable.as_millis() as u64;
                self.tokio_handle.spawn(async move {
                    let millis_to_sleep = thread_rng().gen_range(min, min * 5) as u64;
                    tokio::time::sleep(Duration::from_millis(millis_to_sleep)).await;
                    forwarding_channel_manager.process_pending_htlc_forwards();
                });
            }
            Event::SpendableOutputs { outputs: _ } => {
                /* todo implement

                               let destination_address = self
                                   .tokio_handle
                                   .block_on(self.bitcoind_client.get_new_address());
                               let output_descriptors = &outputs.iter().collect::<Vec<_>>();
                               let tx_feerate = self
                                   .bitcoind_client
                                   .get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
                               let spending_tx = self
                                   .keys_manager
                                   .spend_spendable_outputs(
                                       output_descriptors,
                                       Vec::new(),
                                       destination_address.script_pubkey(),
                                       tx_feerate,
                                       &Secp256k1::new(),
                                   )
                                   .unwrap();
                               self.bitcoind_client.broadcast_transaction(&spending_tx);

                */
            }
            Event::ChannelClosed {
                channel_id,
                reason,
                user_channel_id: _,
            } => {
                info!(
                    "\nEVENT: Channel {} closed due to: {:?}",
                    hex_utils::hex_str(channel_id),
                    reason
                );
            }
            Event::DiscardFunding { .. } => {
                // A "real" node should probably "lock" the UTXOs spent in funding transactions until
                // the funding transaction either confirms, or this event is generated.
            }
        }
    }
}
