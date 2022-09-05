use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};

pub struct ElectrumClient;

impl FeeEstimator for ElectrumClient {
    fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
        // TODO: Implement.
        match confirmation_target {
            ConfirmationTarget::Background => 253,
            ConfirmationTarget::Normal => 1000,
            ConfirmationTarget::HighPriority => 2000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_is_above_minimum() {
        let client = ElectrumClient {};
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Background) >= 253);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) >= 253);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority) >= 253);
    }

    #[test]
    fn fee_is_reasonable() {
        let client = ElectrumClient {};
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Background) < 1000000);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) < 5000000);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority) < 10000000);
    }

    #[test]
    fn fee_is_ordered() {
        let client = ElectrumClient {};
        let background = client.get_est_sat_per_1000_weight(ConfirmationTarget::Background);
        let normal = client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
        let high_priority = client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority);
        assert!(background <= normal);
        assert!(normal <= high_priority);
    }
}
