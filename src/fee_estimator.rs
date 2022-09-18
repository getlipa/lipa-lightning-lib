use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};

#[derive(Debug)]
pub struct FeeEstimatorDummy {}

impl FeeEstimator for FeeEstimatorDummy {
    fn get_est_sat_per_1000_weight(&self, _confirmation_target: ConfirmationTarget) -> u32 {
        // todo
        10000u32
    }
}
