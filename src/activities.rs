use crate::amount::{AsSats, ToAmount};
use crate::data_store::CreatedInvoice;
use crate::errors::Result;
use crate::locker::Locker;
use crate::node_config::WithTimezone;
use crate::support::Support;
use crate::util::unix_timestamp_to_system_time;
use crate::{
    fill_payout_fee, filter_out_and_log_corrupted_activities,
    filter_out_and_log_corrupted_payments, Activity, ChannelCloseInfo, ChannelCloseState,
    IncomingPaymentInfo, InvoiceDetails, ListActivitiesResponse, OutgoingPaymentInfo, PaymentInfo,
    PaymentState, ReverseSwapInfo, RuntimeErrorCode, SwapInfo,
};
use breez_sdk_core::{
    parse_invoice, ClosedChannelPaymentDetails, ListPaymentsRequest, PaymentDetails, PaymentStatus,
    PaymentTypeFilter,
};
use perro::{invalid_input, permanent_failure, MapToError, OptionToError};
use std::cmp::{min, Reverse};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

pub struct Activities {
    support: Arc<Support>,
}

impl Activities {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// List the latest activities
    ///
    /// Parameters:
    /// * `number_of_completed_activities` - the maximum number of completed activities that will be returned
    ///
    /// Requires network: **no**
    pub fn list(&self, number_of_completed_activities: u32) -> Result<ListActivitiesResponse> {
        const LEEWAY_FOR_PENDING_PAYMENTS: u32 = 30;
        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![
                PaymentTypeFilter::Sent,
                PaymentTypeFilter::Received,
                PaymentTypeFilter::ClosedChannel,
            ]),
            metadata_filters: None,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(true),
            limit: Some(number_of_completed_activities + LEEWAY_FOR_PENDING_PAYMENTS),
            offset: None,
        };
        let breez_activities = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .map(|p| self.activity_from_breez_payment(p))
            .filter_map(filter_out_and_log_corrupted_activities)
            .collect::<Vec<_>>();

        // Query created invoices, filter out ones which are in the breez db.
        let created_invoices = self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_created_invoices(number_of_completed_activities)?;

        let number_of_created_invoices = created_invoices.len();
        let mut activities = self.multiplex_activities(breez_activities, created_invoices);
        activities.sort_by_cached_key(|m| Reverse(m.get_time()));

        // To produce stable output we look for pending activities only in the
        // first `look_for_pending` latest activities.
        // Yes, we risk to omit old pending ones.
        let look_for_pending = LEEWAY_FOR_PENDING_PAYMENTS as usize + number_of_created_invoices;
        let mut tail_activities = activities.split_off(min(look_for_pending, activities.len()));
        let head_activities = activities;
        let (mut pending_activities, mut completed_activities): (Vec<_>, Vec<_>) =
            head_activities.into_iter().partition(Activity::is_pending);
        tail_activities.retain(|m| !m.is_pending());
        completed_activities.append(&mut tail_activities);
        completed_activities.truncate(number_of_completed_activities as usize);

        if let Some(in_progress_swap) = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.in_progress_swap())
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get in-progress swap",
            )?
        {
            let created_at = unix_timestamp_to_system_time(in_progress_swap.created_at as u64)
                .with_timezone(
                    self.support
                        .user_preferences
                        .lock_unwrap()
                        .clone()
                        .timezone_config,
                );

            pending_activities.push(Activity::Swap {
                incoming_payment_info: None,
                swap_info: SwapInfo {
                    bitcoin_address: in_progress_swap.bitcoin_address,
                    created_at,
                    // Multiple txs can be sent to swap address and they aren't guaranteed to
                    // confirm all at the same time. Our best guess of the amount that will be
                    // received once the entire swap confirms is given by confirmed sats added to
                    // any unconfirmed sats waiting to be confirmed.
                    paid_amount: (in_progress_swap.unconfirmed_sats
                        + in_progress_swap.confirmed_sats)
                        .as_sats()
                        .to_amount_down(&self.support.get_exchange_rate()),
                },
            })
        }
        pending_activities.sort_by_cached_key(|m| Reverse(m.get_time()));

        Ok(ListActivitiesResponse {
            pending_activities,
            completed_activities,
        })
    }

    /// Get an activity by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    pub fn get(&self, hash: String) -> Result<Activity> {
        if let Some(activity) = self
            .support
            .rt
            .handle()
            .block_on(self.support.sdk.payment_by_hash(hash.clone()))
            .map_to_runtime_error(
                RuntimeErrorCode::NodeUnavailable,
                "Failed to get payment by hash",
            )?
            .map(|p| self.activity_from_breez_payment(p))
        {
            return activity;
        }

        let invoice = self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_created_invoice_by_hash(&hash)?;
        if let Some(invoice) = invoice {
            let incoming_payment_info = self.payment_from_created_invoice(&invoice);
            Ok(Activity::IncomingPayment {
                incoming_payment_info: incoming_payment_info?,
            })
        } else {
            invalid_input!("No activity with provided hash was found")
        }
    }

    /// Get a reverse swap activity by reverse swap id.
    ///
    /// Parameters:
    /// * `reverse_swap_id` - the id of a reverse swap.
    ///
    /// Requires network: **no**
    pub fn get_by_reverse_swap(&self, reverse_swap_id: String) -> Result<Option<Activity>> {
        const LEEWAY_FOR_REVERSE_SWAPS: u32 = 30;
        let list_payments_request = ListPaymentsRequest {
            filters: Some(vec![PaymentTypeFilter::Sent]),
            metadata_filters: None,
            from_timestamp: None,
            to_timestamp: None,
            include_failures: Some(false),
            limit: Some(LEEWAY_FOR_REVERSE_SWAPS),
            offset: None,
        };

        let is_swap_with_id = |p: &breez_sdk_core::Payment| {
            if let breez_sdk_core::PaymentDetails::Ln { ref data } = p.details {
                if let Some(ref swap_info) = data.reverse_swap_info {
                    return swap_info.id == reverse_swap_id;
                }
            }
            false
        };
        self.support
            .rt
            .handle()
            .block_on(self.support.sdk.list_payments(list_payments_request))
            .map_to_runtime_error(RuntimeErrorCode::NodeUnavailable, "Failed to list payments")?
            .into_iter()
            .find(is_swap_with_id)
            .map(|p| self.activity_from_breez_payment(p))
            .transpose()
    }

    /// Get an incoming payment by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    pub fn get_incoming_payment(&self, hash: String) -> Result<IncomingPaymentInfo> {
        match self.get(hash)? {
            Activity::IncomingPayment {
                incoming_payment_info,
            } => Ok(incoming_payment_info),
            Activity::OfferClaim {
                incoming_payment_info,
                ..
            } => Ok(incoming_payment_info),
            Activity::Swap {
                incoming_payment_info,
                ..
            } => incoming_payment_info
                .ok_or(invalid_input("Swap activity without incoming payment info")),
            Activity::ReverseSwap { .. }
            | Activity::OutgoingPayment { .. }
            | Activity::ChannelClose { .. } => invalid_input!("Activity not incoming payment"),
        }
    }

    /// Get an outgoing payment by its payment hash.
    ///
    /// Parameters:
    /// * `hash` - hex representation of payment hash
    ///
    /// Requires network: **no**
    pub fn get_outgoing_payment(&self, hash: String) -> Result<OutgoingPaymentInfo> {
        match self.get(hash)? {
            Activity::OutgoingPayment {
                outgoing_payment_info,
            } => Ok(outgoing_payment_info),
            Activity::ReverseSwap {
                outgoing_payment_info,
                ..
            } => Ok(outgoing_payment_info),
            Activity::OfferClaim { .. }
            | Activity::IncomingPayment { .. }
            | Activity::ChannelClose { .. }
            | Activity::Swap { .. } => invalid_input!("Activity not incoming payment"),
        }
    }

    /// Set a personal note on a specific activity. Can only be used for activities that can be
    /// identified by a payment hash (e.g. channel closes are excluded).
    ///
    /// Parameters:
    /// * `payment_hash` - The hash of the activity for which a personal note will be set.
    /// * `note` - The personal note.
    ///
    /// Requires network: **no**
    pub fn set_personal_note(&self, payment_hash: String, note: String) -> Result<()> {
        let note = Some(note.trim().to_string()).filter(|s| !s.is_empty());

        self.support
            .data_store
            .lock_unwrap()
            .update_personal_note(&payment_hash, note.as_deref())
    }

    pub(crate) fn activity_from_breez_payment(
        &self,
        breez_payment: breez_sdk_core::Payment,
    ) -> Result<Activity> {
        match &breez_payment.details {
            PaymentDetails::Ln { .. } => self.activity_from_breez_ln_payment(breez_payment),
            PaymentDetails::ClosedChannel { data } => {
                self.activity_from_breez_closed_channel_payment(&breez_payment, data)
            }
        }
    }

    /// Combines a list of activities with a list of locally created invoices
    /// into a single activity list.
    ///
    /// Duplicates are removed.
    fn multiplex_activities(
        &self,
        breez_activities: Vec<Activity>,
        local_created_invoices: Vec<CreatedInvoice>,
    ) -> Vec<Activity> {
        let breez_payment_hashes: HashSet<_> = breez_activities
            .iter()
            .filter_map(|m| m.get_payment_info().map(|p| p.hash.clone()))
            .collect();
        let mut activities = local_created_invoices
            .into_iter()
            .filter(|i| !breez_payment_hashes.contains(i.hash.as_str()))
            .map(|i| self.payment_from_created_invoice(&i))
            .filter_map(filter_out_and_log_corrupted_payments)
            .map(|p| Activity::IncomingPayment {
                incoming_payment_info: p,
            })
            .collect::<Vec<_>>();
        activities.extend(breez_activities);
        activities
    }

    pub(crate) fn activity_from_breez_ln_payment(
        &self,
        breez_payment: breez_sdk_core::Payment,
    ) -> Result<Activity> {
        let payment_details = match breez_payment.details {
            PaymentDetails::Ln { ref data } => data,
            PaymentDetails::ClosedChannel { .. } => {
                invalid_input!("PaymentInfo cannot be created from channel close")
            }
        };
        let local_payment_data = self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_payment_info(&payment_details.payment_hash)?;
        let (exchange_rate, tz_config, personal_note, offer, received_on, received_lnurl_comment) =
            match local_payment_data {
                Some(data) => (
                    Some(data.exchange_rate),
                    data.user_preferences.timezone_config,
                    data.personal_note,
                    data.offer,
                    data.received_on,
                    data.received_lnurl_comment,
                ),
                None => (
                    self.support.get_exchange_rate(),
                    self.support
                        .user_preferences
                        .lock_unwrap()
                        .timezone_config
                        .clone(),
                    None,
                    None,
                    None,
                    None,
                ),
            };

        if let Some(offer) = offer {
            let incoming_payment_info = IncomingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                received_on,
                received_lnurl_comment,
                &self
                    .support
                    .node_config
                    .remote_services_config
                    .lipa_lightning_domain,
            )?;
            let offer = fill_payout_fee(
                offer,
                incoming_payment_info.requested_amount.sats.as_msats(),
                &exchange_rate,
            );
            Ok(Activity::OfferClaim {
                incoming_payment_info,
                offer,
            })
        } else if let Some(ref s) = payment_details.swap_info {
            let swap_info = SwapInfo {
                bitcoin_address: s.bitcoin_address.clone(),
                // TODO: Persist SwapInfo in local db on state change, requires https://github.com/breez/breez-sdk/issues/518
                created_at: unix_timestamp_to_system_time(s.created_at as u64)
                    .with_timezone(tz_config.clone()),
                paid_amount: s.paid_msat.as_msats().to_amount_down(&exchange_rate),
            };
            let incoming_payment_info = IncomingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                received_on,
                received_lnurl_comment,
                &self
                    .support
                    .node_config
                    .remote_services_config
                    .lipa_lightning_domain,
            )?;
            Ok(Activity::Swap {
                incoming_payment_info: Some(incoming_payment_info),
                swap_info,
            })
        } else if let Some(ref s) = payment_details.reverse_swap_info {
            let reverse_swap_info = ReverseSwapInfo {
                paid_onchain_amount: s.onchain_amount_sat.as_sats().to_amount_up(&exchange_rate),
                swap_fees_amount: (breez_payment.amount_msat
                    - s.onchain_amount_sat.as_sats().msats)
                    .as_msats()
                    .to_amount_up(&exchange_rate),
                claim_txid: s.claim_txid.clone(),
                status: s.status,
            };
            let outgoing_payment_info = OutgoingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                &self
                    .support
                    .node_config
                    .remote_services_config
                    .lipa_lightning_domain,
            )?;
            Ok(Activity::ReverseSwap {
                outgoing_payment_info,
                reverse_swap_info,
            })
        } else if breez_payment.payment_type == breez_sdk_core::PaymentType::Received {
            let incoming_payment_info = IncomingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                received_on,
                received_lnurl_comment,
                &self
                    .support
                    .node_config
                    .remote_services_config
                    .lipa_lightning_domain,
            )?;
            Ok(Activity::IncomingPayment {
                incoming_payment_info,
            })
        } else if breez_payment.payment_type == breez_sdk_core::PaymentType::Sent {
            let outgoing_payment_info = OutgoingPaymentInfo::new(
                breez_payment,
                &exchange_rate,
                tz_config,
                personal_note,
                &self
                    .support
                    .node_config
                    .remote_services_config
                    .lipa_lightning_domain,
            )?;
            Ok(Activity::OutgoingPayment {
                outgoing_payment_info,
            })
        } else {
            permanent_failure!("Unreachable code")
        }
    }

    pub(crate) fn activity_from_breez_closed_channel_payment(
        &self,
        breez_payment: &breez_sdk_core::Payment,
        details: &ClosedChannelPaymentDetails,
    ) -> Result<Activity> {
        let amount = breez_payment
            .amount_msat
            .as_msats()
            .to_amount_up(&self.support.get_exchange_rate());

        let user_preferences = self.support.user_preferences.lock_unwrap();

        let time = unix_timestamp_to_system_time(breez_payment.payment_time as u64)
            .with_timezone(user_preferences.timezone_config.clone());

        let (closed_at, state) = match breez_payment.status {
            PaymentStatus::Pending => (None, ChannelCloseState::Pending),
            PaymentStatus::Complete => (Some(time), ChannelCloseState::Confirmed),
            PaymentStatus::Failed => {
                permanent_failure!("A channel close Breez Payment has status *Failed*");
            }
        };

        // According to the docs, it can only be empty for older closed channels.
        let closing_tx_id = details.closing_txid.clone().unwrap_or_default();

        Ok(Activity::ChannelClose {
            channel_close_info: ChannelCloseInfo {
                amount,
                state,
                closed_at,
                closing_tx_id,
            },
        })
    }

    fn payment_from_created_invoice(
        &self,
        created_invoice: &CreatedInvoice,
    ) -> Result<IncomingPaymentInfo> {
        let invoice =
            parse_invoice(created_invoice.invoice.as_str()).map_to_permanent_failure(format!(
                "Invalid invoice obtained from local db: {}",
                created_invoice.invoice
            ))?;
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice.clone(), &None);

        let payment_state = if SystemTime::now() > invoice_details.expiry_timestamp {
            PaymentState::InvoiceExpired
        } else {
            PaymentState::Created
        };

        let local_payment_data = self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_payment_info(&invoice_details.payment_hash)?
            .ok_or_permanent_failure("Locally created invoice doesn't have local payment data")?;
        let exchange_rate = Some(local_payment_data.exchange_rate);
        let invoice_details = InvoiceDetails::from_ln_invoice(invoice, &exchange_rate);
        // For receiving payments, we use the invoice timestamp.
        let time = invoice_details
            .creation_timestamp
            .with_timezone(local_payment_data.user_preferences.timezone_config);
        let lsp_fees = created_invoice
            .channel_opening_fees
            .unwrap_or_default()
            .as_msats()
            .to_amount_up(&exchange_rate);
        let requested_amount = invoice_details
            .amount
            .clone()
            .ok_or_permanent_failure("Locally created invoice doesn't include an amount")?
            .sats
            .as_sats()
            .to_amount_down(&exchange_rate);

        let amount = requested_amount.clone().sats - lsp_fees.sats;
        let amount = amount.as_sats().to_amount_down(&exchange_rate);

        let personal_note = local_payment_data.personal_note;

        let payment_info = PaymentInfo {
            payment_state,
            hash: invoice_details.payment_hash.clone(),
            amount,
            invoice_details: invoice_details.clone(),
            created_at: time,
            description: invoice_details.description,
            preimage: None,
            personal_note,
        };
        let incoming_payment_info = IncomingPaymentInfo {
            payment_info,
            requested_amount,
            lsp_fees,
            received_on: None,
            received_lnurl_comment: None,
        };
        Ok(incoming_payment_info)
    }
}
