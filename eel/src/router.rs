use crate::types::Router;
use lightning::ln::channelmanager::{ChannelDetails, PaymentId};
use lightning::ln::msgs::{ErrorAction, LightningError};
use lightning::ln::PaymentHash;
use lightning::routing::router::{InFlightHtlcs, Route, RouteParameters};
use secp256k1::PublicKey;
use std::sync::Arc;

pub enum MaxRoutingFeeMode {
    Relative { max_fee_permyriad: u16 },
    Absolute { max_fee_msat: u64 },
}

pub(crate) struct SimpleMaxRoutingFeeStrategy {
    min_max_fee_msat: u64,
    max_relative_fee_permyriad: u16,
}

impl SimpleMaxRoutingFeeStrategy {
    pub fn new(min_max_fee_msat: u64, max_relative_fee_permyriad: u16) -> Self {
        SimpleMaxRoutingFeeStrategy {
            min_max_fee_msat,
            max_relative_fee_permyriad,
        }
    }

    pub fn get_payment_max_fee_mode(&self, payment_amount_msat: u64) -> MaxRoutingFeeMode {
        let threshold = self.min_max_fee_msat * 10_000 / self.max_relative_fee_permyriad as u64;

        if payment_amount_msat > threshold {
            MaxRoutingFeeMode::Relative {
                max_fee_permyriad: self.max_relative_fee_permyriad,
            }
        } else {
            MaxRoutingFeeMode::Absolute {
                max_fee_msat: self.min_max_fee_msat,
            }
        }
    }

    pub fn compute_max_fee_msat(&self, payment_amount_msat: u64) -> u64 {
        match self.get_payment_max_fee_mode(payment_amount_msat) {
            MaxRoutingFeeMode::Relative { max_fee_permyriad } => {
                payment_amount_msat * max_fee_permyriad as u64 / 10000
            }
            MaxRoutingFeeMode::Absolute { max_fee_msat } => max_fee_msat,
        }
    }
}

pub(crate) struct FeeLimitingRouter {
    inner: Router,
    max_fee_strategy: Arc<SimpleMaxRoutingFeeStrategy>,
}

impl FeeLimitingRouter {
    pub fn new(router: Router, max_fee_strategy: Arc<SimpleMaxRoutingFeeStrategy>) -> Self {
        FeeLimitingRouter {
            inner: router,
            max_fee_strategy,
        }
    }
}

impl lightning::routing::router::Router for FeeLimitingRouter {
    fn find_route(
        &self,
        payer: &PublicKey,
        route_params: &RouteParameters,
        first_hops: Option<&[&ChannelDetails]>,
        inflight_htlcs: &InFlightHtlcs,
    ) -> Result<Route, LightningError> {
        let max_fee_msat = self
            .max_fee_strategy
            .compute_max_fee_msat(route_params.final_value_msat);

        let route = self
            .inner
            .find_route(payer, route_params, first_hops, inflight_htlcs)?;
        let route_fees = route.get_total_fees();
        if route_fees > max_fee_msat {
            Err(LightningError {
                err: format!("Route's fees exceed maximum allowed - max allowed: {max_fee_msat} - route's fees {route_fees}"),
                action: ErrorAction::IgnoreError,
            })
        } else {
            Ok(route)
        }
    }

    fn find_route_with_id(
        &self,
        payer: &PublicKey,
        route_params: &RouteParameters,
        first_hops: Option<&[&ChannelDetails]>,
        inflight_htlcs: &InFlightHtlcs,
        payment_hash: PaymentHash,
        payment_id: PaymentId,
    ) -> Result<Route, LightningError> {
        let max_fee_msat = self
            .max_fee_strategy
            .compute_max_fee_msat(route_params.final_value_msat);

        let route = self.inner.find_route_with_id(
            payer,
            route_params,
            first_hops,
            inflight_htlcs,
            payment_hash,
            payment_id,
        )?;
        let route_fees = route.get_total_fees();
        if route_fees > max_fee_msat {
            Err(LightningError {
                err: format!("Route's fees exceed maximum allowed - max allowed: {max_fee_msat} - route's fees {route_fees}"),
                action: ErrorAction::IgnoreError,
            })
        } else {
            Ok(route)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::router::SimpleMaxRoutingFeeStrategy;

    #[test]
    fn test_simple_max_routing_fee_strategy() {
        let max_fee_strategy = SimpleMaxRoutingFeeStrategy::new(21000, 50);

        assert_eq!(max_fee_strategy.compute_max_fee_msat(0), 21_000);
        assert_eq!(max_fee_strategy.compute_max_fee_msat(21_000), 21_000);
        assert_eq!(max_fee_strategy.compute_max_fee_msat(4199_000), 21_000);
        assert_eq!(max_fee_strategy.compute_max_fee_msat(4200_000), 21_000);
        assert_eq!(max_fee_strategy.compute_max_fee_msat(4201_000), 21_005);
        assert_eq!(max_fee_strategy.compute_max_fee_msat(4399_000), 21_995);
        assert_eq!(max_fee_strategy.compute_max_fee_msat(4400_000), 22_000);
    }
}
