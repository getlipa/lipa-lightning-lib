use crate::payment::{IncomingPaymentInfo, OutgoingPaymentInfo, PaymentInfo};
use crate::{Amount, OfferKind, SwapInfo, TzTime};

use std::time::SystemTime;

/// Information about **all** pending and **only** requested completed activities.
pub struct ListActivitiesResponse {
    pub pending_activities: Vec<Activity>,
    pub completed_activities: Vec<Activity>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum Activity {
    IncomingPayment {
        incoming_payment_info: IncomingPaymentInfo,
    },
    OutgoingPayment {
        outgoing_payment_info: OutgoingPaymentInfo,
    },
    // Topup, referrals.
    OfferClaim {
        incoming_payment_info: IncomingPaymentInfo,
        offer_kind: OfferKind,
    },
    // Lightning to an on-chain address.
    ReverseSwap {
        outgoing_payment_info: OutgoingPaymentInfo,
        // reverse_swap_info: ReverseSwapInfo,
    },
    // On-chain to lightning.
    Swap {
        incoming_payment_info: IncomingPaymentInfo,
        swap_info: SwapInfo,
    },
    ChannelClose {
        channel_close_info: ChannelCloseInfo,
    },
    // On-chain to an external on-chain address.
    // Sweep {
    //     sweep_info: SweepInfo,
    // },
}

impl Activity {
    pub(crate) fn get_payment_info(&self) -> Option<&PaymentInfo> {
        match self {
            Activity::IncomingPayment {
                incoming_payment_info,
            } => Some(&incoming_payment_info.payment_info),
            Activity::OutgoingPayment {
                outgoing_payment_info,
            } => Some(&outgoing_payment_info.payment_info),
            Activity::OfferClaim {
                incoming_payment_info,
                ..
            } => Some(&incoming_payment_info.payment_info),
            Activity::ReverseSwap {
                outgoing_payment_info,
            } => Some(&outgoing_payment_info.payment_info),
            Activity::Swap {
                incoming_payment_info,
                ..
            } => Some(&incoming_payment_info.payment_info),
            Activity::ChannelClose { .. } => None,
        }
    }

    pub(crate) fn get_time(&self) -> SystemTime {
        if let Some(payment_info) = self.get_payment_info() {
            return payment_info.created_at.time;
        }
        match self {
            Activity::ChannelClose {
                channel_close_info:
                    ChannelCloseInfo {
                        amount: _,
                        state: _,
                        closed_at: Some(time),
                        ..
                    },
            } => time.time,
            _ => SystemTime::now(),
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        if let Some(payment_info) = self.get_payment_info() {
            return payment_info.payment_state.is_pending();
        }
        match self {
            Activity::ChannelClose { channel_close_info } => match channel_close_info.state {
                ChannelCloseState::Pending => true,
                ChannelCloseState::Confirmed => false,
            },
            _ => false,
        }
    }
}

/// Information about a closed channel.
#[derive(Debug, PartialEq)]
pub struct ChannelCloseInfo {
    /// Our balance on the channel that got closed.
    pub amount: Amount,
    pub state: ChannelCloseState,
    /// When the channel closing tx got confirmed. For pending channel closes, this will be empty.
    pub closed_at: Option<TzTime>,
    pub closing_tx_id: String,
}

#[derive(Debug, PartialEq)]
pub enum ChannelCloseState {
    Pending,
    Confirmed,
}
