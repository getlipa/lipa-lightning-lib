use graphql::errors::*;
use honeybadger::Auth;
use std::sync::Arc;
use std::time::SystemTime;

const EXCHANGE_RATE_BASE: u32 = 1_700u32; // sat per EUR/CHF

pub use chameleon::ExchangeRate;
use rand::Rng;

pub struct ExchangeRateProvider {}

impl ExchangeRateProvider {
    pub fn new(_backend_url: String, _auth: Arc<Auth>) -> Self {
        Self {}
    }

    pub fn list_currency_codes(&self) -> Result<Vec<String>> {
        Ok(vec!["CHF".to_string(), "EUR".to_string()])
    }

    pub fn query_exchange_rate(&self, _code: String) -> Result<u32> {
        Ok(get_randomized_exchange_rate())
    }

    pub fn query_all_exchange_rates(&self) -> Result<Vec<ExchangeRate>> {
        Ok(vec![
            ExchangeRate {
                currency_code: "CHF".to_string(),
                sats_per_unit: get_randomized_exchange_rate(),
                updated_at: SystemTime::now(),
            },
            ExchangeRate {
                currency_code: "EUR".to_string(),
                sats_per_unit: get_randomized_exchange_rate(),
                updated_at: SystemTime::now(),
            },
        ])
    }
}

pub fn get_randomized_exchange_rate() -> u32 {
    let mut rng = rand::thread_rng();
    let twenty_percent = EXCHANGE_RATE_BASE / 5;
    let lower = EXCHANGE_RATE_BASE - twenty_percent;
    let upper = EXCHANGE_RATE_BASE + twenty_percent;

    rng.gen_range(lower..upper)
}
