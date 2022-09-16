use lightning::util::events::{Event, EventHandler};

pub struct LipaEventHandler {}

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
            Event::OpenChannelRequest { .. } => {}
            Event::HTLCHandlingFailed { .. } => {}
        }
    }
}
