use crate::errors::Result;
use crate::esplora_client::EsploraClient;
use bitcoin::Network;
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator as LdkFeeEstimator};
use log::debug;
use perro::permanent_failure;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const BACKGROUND_CONFIRM_IN_BLOCKS: &str = "25";
const NORMAL_CONFIRM_IN_BLOCKS: &str = "6";
const HIGH_PRIORITY_CONFIRM_IN_BLOCKS: &str = "1";

const MIN_FEERATE: u32 = 253; // 1 sats per byte

const BACKGROUND_DEFAULT: u32 = MIN_FEERATE; // 1 sats per byte
const NORMAL_DEFAULT: u32 = 2000; // 8 sats per byte
const HIGH_PRIORITY_DEFAULT: u32 = 5000; // 20 sats per byte

pub(crate) struct FeeEstimator {
    esplora_client: Arc<EsploraClient>,
    fees: Arc<HashMap<ConfirmationTarget, AtomicU32>>,
    network: Network,
}

impl FeeEstimator {
    pub fn new(esplora_client: Arc<EsploraClient>, network: Network) -> Self {
        // Init fees
        let mut fees: HashMap<ConfirmationTarget, AtomicU32> = HashMap::new();
        fees.insert(
            ConfirmationTarget::Background,
            AtomicU32::new(BACKGROUND_DEFAULT),
        );
        fees.insert(ConfirmationTarget::Normal, AtomicU32::new(NORMAL_DEFAULT));
        fees.insert(
            ConfirmationTarget::HighPriority,
            AtomicU32::new(HIGH_PRIORITY_DEFAULT),
        );
        let fees = Arc::new(fees);

        Self {
            esplora_client,
            fees,
            network,
        }
    }

    pub fn poll_updates(&self) -> Result<()> {
        if self.network != Network::Bitcoin {
            return Ok(());
        }

        let estimates = self.esplora_client.get_fee_estimates()?;

        let background_estimate =
            get_ldk_estimate_from_esplora_estimates(&estimates, BACKGROUND_CONFIRM_IN_BLOCKS)?;
        let normal_estimate =
            get_ldk_estimate_from_esplora_estimates(&estimates, NORMAL_CONFIRM_IN_BLOCKS)?;
        let high_priority_estimate =
            get_ldk_estimate_from_esplora_estimates(&estimates, HIGH_PRIORITY_CONFIRM_IN_BLOCKS)?;

        // Multi-line print done with a single debug! so that the lines can't
        // get separated by other debug prints
        debug!("FeeEstimator fetched new estimates from esplora:\n    Background: {}\n    Normal: {}\n    HighPriority: {}", background_estimate, normal_estimate, high_priority_estimate);

        self.fees
            .get(&ConfirmationTarget::Background)
            .unwrap()
            .store(background_estimate, Ordering::Release);
        self.fees
            .get(&ConfirmationTarget::Normal)
            .unwrap()
            .store(normal_estimate, Ordering::Release);
        self.fees
            .get(&ConfirmationTarget::HighPriority)
            .unwrap()
            .store(high_priority_estimate, Ordering::Release);

        Ok(())
    }
}

fn get_ldk_estimate_from_esplora_estimates(
    esplora_estimates: &HashMap<String, f64>,
    confirm_in_blocks: &str,
) -> Result<u32> {
    let background_estimate = match esplora_estimates.get(confirm_in_blocks) {
        None => {
            return Err(permanent_failure(format!("Failed to get fee estimates: Esplora didn't provide an estimate for confirmation in {confirm_in_blocks} blocks")));
        }
        Some(e) => e,
    };
    Ok(std::cmp::max(
        (background_estimate * 250.0).round() as u32,
        MIN_FEERATE,
    ))
}

impl LdkFeeEstimator for FeeEstimator {
    fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
        self.fees
            .get(&confirmation_target)
            .unwrap()
            .load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 9 is a discard port
    // See https://en.wikipedia.org/wiki/Port_(computer_networking)
    const DISCARD_ESPLORA_API_URL: &str = "http://localhost:9";

    const MAINNET_ESPLORA_API_URL: &str = "https://blockstream.info/api/";

    #[test]
    fn fee_is_above_minimum() {
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(DISCARD_ESPLORA_API_URL).unwrap()),
            Network::Bitcoin,
        );
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Background) >= 253);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) >= 253);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority) >= 253);
    }

    #[test]
    fn fee_is_reasonable() {
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(DISCARD_ESPLORA_API_URL).unwrap()),
            Network::Bitcoin,
        );
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Background) < 1000000);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) < 5000000);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority) < 10000000);
    }

    #[test]
    fn fee_is_ordered() {
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(DISCARD_ESPLORA_API_URL).unwrap()),
            Network::Bitcoin,
        );
        let background = client.get_est_sat_per_1000_weight(ConfirmationTarget::Background);
        let normal = client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
        let high_priority = client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority);
        assert!(background <= normal);
        assert!(normal <= high_priority);
    }

    #[test]
    fn can_get_mainnet_fee_estimations() {
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(MAINNET_ESPLORA_API_URL).unwrap()),
            Network::Bitcoin,
        );
        // If poll_updates() is successful, it means the response from esplora included
        // the fee estimations we need
        client.poll_updates().unwrap();
    }
}
