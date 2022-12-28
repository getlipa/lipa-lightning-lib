use crate::async_runtime::Handle;
use crate::esplora_client::EsploraClient;
use bitcoin::Network;
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator as LdkFeeEstimator};
use log::{debug, error};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FEE_ESTIMATE_POLLING_INTERVAL: u64 = 60;

const BACKGROUND_CONFIRM_IN_BLOCKS: &str = "25";
const NORMAL_CONFIRM_IN_BLOCKS: &str = "6";
const HIGH_PRIORITY_CONFIRM_IN_BLOCKS: &str = "1";

const MIN_FEERATE: u32 = 253; // 1 sats per byte

const BACKGROUND_DEFAULT: u32 = MIN_FEERATE; // 1 sats per byte
const NORMAL_DEFAULT: u32 = 2000; // 8 sats per byte
const HIGH_PRIORITY_DEFAULT: u32 = 5000; // 20 sats per byte

pub(crate) struct FeeEstimator {
    fees: Arc<HashMap<ConfirmationTarget, AtomicU32>>,
    network: Network,
}

impl FeeEstimator {
    pub fn new(
        esplora_client: Arc<EsploraClient>,
        runtime_handle: Handle,
        network: Network,
    ) -> Self {
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

        // Launch polling background task
        let esplora_client_poll = Arc::clone(&esplora_client);
        let fees_poll = Arc::clone(&fees);
        runtime_handle.spawn_repeating_task(
            Duration::from_secs(FEE_ESTIMATE_POLLING_INTERVAL),
            move || {
                let esplora_client_poll = Arc::clone(&esplora_client_poll);
                let fees_poll = Arc::clone(&fees_poll);
                async move {
                    match esplora_client_poll.get_fee_estimates() {
                        Ok(estimates) => {
                            let background_estimate = get_ldk_estimate_from_esplora_estimates(
                                &estimates,
                                BACKGROUND_CONFIRM_IN_BLOCKS,
                                BACKGROUND_DEFAULT,
                            );
                            let normal_estimate = get_ldk_estimate_from_esplora_estimates(
                                &estimates,
                                NORMAL_CONFIRM_IN_BLOCKS,
                                NORMAL_DEFAULT,
                            );
                            let high_priority_estimate = get_ldk_estimate_from_esplora_estimates(
                                &estimates,
                                HIGH_PRIORITY_CONFIRM_IN_BLOCKS,
                                HIGH_PRIORITY_DEFAULT,
                            );

                            // Multi-line print done with a single debug! so that the lines can't 
                            // get separated by other debug prints
                            debug!("FeeEstimator fetched new estimates from esplora: \n    Background: {}\n    Normal: {} \n    HighPriority: {}", background_estimate, normal_estimate, high_priority_estimate);

                            fees_poll
                                .get(&ConfirmationTarget::Background)
                                .unwrap()
                                .store(background_estimate, Ordering::Release);
                            fees_poll
                                .get(&ConfirmationTarget::Normal)
                                .unwrap()
                                .store(normal_estimate, Ordering::Release);
                            fees_poll
                                .get(&ConfirmationTarget::HighPriority)
                                .unwrap()
                                .store(high_priority_estimate, Ordering::Release);
                        }
                        Err(e) => {
                            error!("Failed to get fee estimates from esplora: {}", e);
                        }
                    }
                }
            },
        );

        Self { fees, network }
    }
}

fn get_ldk_estimate_from_esplora_estimates(
    esplora_estimates: &HashMap<String, f64>,
    confirm_in_blocks: &str,
    default: u32,
) -> u32 {
    let background_estimate = match esplora_estimates.get(confirm_in_blocks) {
        None => {
            error!("Failed to get fee estimates: Esplora didn't provide an estimate for confirmation in {} blocks", confirm_in_blocks);
            return default;
        }
        Some(e) => e,
    };
    std::cmp::max((background_estimate * 250.0).round() as u32, MIN_FEERATE)
}

impl LdkFeeEstimator for FeeEstimator {
    fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
        match self.network {
            Network::Bitcoin => self
                .fees
                .get(&confirmation_target)
                .unwrap()
                .load(Ordering::Acquire),
            _ => match confirmation_target {
                ConfirmationTarget::Background => BACKGROUND_DEFAULT,
                ConfirmationTarget::Normal => NORMAL_DEFAULT,
                ConfirmationTarget::HighPriority => HIGH_PRIORITY_DEFAULT,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_runtime::AsyncRuntime;

    // 9 is a discard port
    // See https://en.wikipedia.org/wiki/Port_(computer_networking)
    const ESPLORA_API_URL: &str = "http://localhost:9";

    #[test]
    fn fee_is_above_minimum() {
        let rt = AsyncRuntime::new().unwrap();
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(ESPLORA_API_URL).unwrap()),
            rt.handle(),
            Network::Bitcoin,
        );
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Background) >= 253);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) >= 253);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority) >= 253);
    }

    #[test]
    fn fee_is_reasonable() {
        let rt = AsyncRuntime::new().unwrap();
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(ESPLORA_API_URL).unwrap()),
            rt.handle(),
            Network::Bitcoin,
        );
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Background) < 1000000);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) < 5000000);
        assert!(client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority) < 10000000);
    }

    #[test]
    fn fee_is_ordered() {
        let rt = AsyncRuntime::new().unwrap();
        let client = FeeEstimator::new(
            Arc::new(EsploraClient::new(ESPLORA_API_URL).unwrap()),
            rt.handle(),
            Network::Bitcoin,
        );
        let background = client.get_est_sat_per_1000_weight(ConfirmationTarget::Background);
        let normal = client.get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
        let high_priority = client.get_est_sat_per_1000_weight(ConfirmationTarget::HighPriority);
        assert!(background <= normal);
        assert!(normal <= high_priority);
    }
}
