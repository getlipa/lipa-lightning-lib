use crate::logger::LightningLogger;
use crate::types::{NetworkGraph, Router, Scorer};
use lightning::ln::channelmanager::{ChannelDetails, PaymentId};
use lightning::ln::msgs::{ErrorAction, LightningError};
use lightning::ln::PaymentHash;
use lightning::routing::router::{InFlightHtlcs, Route, RouteParameters};
use secp256k1::PublicKey;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

pub(crate) trait MaxRoutingFeeProvider {
    fn compute_max_fee_msat(&self, payment_amount_msat: u64) -> u64;
}

pub enum MaxFeeStrategy {
    Relative { max_fee_permyriad: u16 },
    Absolute { max_fee_msat: u64 },
}

pub(crate) struct SimpleMaxRoutingFeeProvider {
    min_max_fee_msat: u64,
    max_relative_fee_permyriad: u16,
}

impl SimpleMaxRoutingFeeProvider {
    pub fn new(min_max_fee_msat: u64, max_relative_fee_permyriad: u16) -> Self {
        SimpleMaxRoutingFeeProvider {
            min_max_fee_msat,
            max_relative_fee_permyriad,
        }
    }

    pub fn get_max_fee_strategy(&self, payment_amount_msat: u64) -> MaxFeeStrategy {
        let threshold = self.min_max_fee_msat * 10_000 / self.max_relative_fee_permyriad as u64;

        if payment_amount_msat > threshold {
            MaxFeeStrategy::Relative {
                max_fee_permyriad: self.max_relative_fee_permyriad,
            }
        } else {
            MaxFeeStrategy::Absolute {
                max_fee_msat: self.min_max_fee_msat,
            }
        }
    }
}

impl MaxRoutingFeeProvider for SimpleMaxRoutingFeeProvider {
    fn compute_max_fee_msat(&self, payment_amount_msat: u64) -> u64 {
        match self.get_max_fee_strategy(payment_amount_msat) {
            MaxFeeStrategy::Relative { max_fee_permyriad } => {
                payment_amount_msat * max_fee_permyriad as u64 / 10000
            }
            MaxFeeStrategy::Absolute { max_fee_msat } => max_fee_msat,
        }
    }
}

pub(crate) struct FeeCappedRouter<MFP: Deref>
where
    MFP::Target: MaxRoutingFeeProvider,
{
    inner: Router,
    max_fee_provider: MFP,
}

impl<MFP: Deref> FeeCappedRouter<MFP>
where
    MFP::Target: MaxRoutingFeeProvider,
{
    pub fn new(
        network_graph: Arc<NetworkGraph>,
        logger: Arc<LightningLogger>,
        random_seed_bytes: [u8; 32],
        scorer: Arc<Mutex<Scorer>>,
        max_fee_provider: MFP,
    ) -> Self {
        let inner = Router::new(
            Arc::clone(&network_graph),
            Arc::clone(&logger),
            random_seed_bytes,
            Arc::clone(&scorer),
        );
        FeeCappedRouter {
            inner,
            max_fee_provider,
        }
    }
}

impl<MFP: Deref> lightning::routing::router::Router for FeeCappedRouter<MFP>
where
    MFP::Target: MaxRoutingFeeProvider,
{
    fn find_route(
        &self,
        payer: &PublicKey,
        route_params: &RouteParameters,
        first_hops: Option<&[&ChannelDetails]>,
        inflight_htlcs: &InFlightHtlcs,
    ) -> Result<Route, LightningError> {
        let max_fee_msat = self
            .max_fee_provider
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
            .max_fee_provider
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
    use crate::router::{MaxRoutingFeeProvider, SimpleMaxRoutingFeeProvider};

    #[test]
    fn test_simple_max_routing_fee_provider() {
        let max_fee_provider = SimpleMaxRoutingFeeProvider::new(21000, 50);

        assert_eq!(max_fee_provider.compute_max_fee_msat(0), 21_000);
        assert_eq!(max_fee_provider.compute_max_fee_msat(21_000), 21_000);
        assert_eq!(max_fee_provider.compute_max_fee_msat(4199_000), 21_000);
        assert_eq!(max_fee_provider.compute_max_fee_msat(4200_000), 21_000);
        assert_eq!(max_fee_provider.compute_max_fee_msat(4201_000), 21_005);
        assert_eq!(max_fee_provider.compute_max_fee_msat(4399_000), 21_995);
        assert_eq!(max_fee_provider.compute_max_fee_msat(4400_000), 22_000);
    }
}
