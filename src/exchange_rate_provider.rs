use crate::errors::Result;
use crate::RuntimeErrorCode;
use honey_badger::Auth;
use perro::ResultTrait;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ExchangeRate {
    pub currency_code: String,
    pub rate: u32,
    pub updated_at: SystemTime,
}

pub trait ExchangeRateProvider: Send + Sync {
    fn query_all_exchange_rates(&self) -> Result<Vec<ExchangeRate>>;
}

pub struct ExchangeRateProviderImpl {
    provider: chameleon::ExchangeRateProvider,
}

impl ExchangeRateProviderImpl {
    pub fn new(graphql_url: String, auth: Arc<Auth>) -> Self {
        let provider = chameleon::ExchangeRateProvider::new(graphql_url, auth);
        Self { provider }
    }
}

impl ExchangeRateProvider for ExchangeRateProviderImpl {
    fn query_all_exchange_rates(&self) -> Result<Vec<ExchangeRate>> {
        Ok(self
            .provider
            .query_all_exchange_rates()
            .map_runtime_error_to(RuntimeErrorCode::ExchangeRateProviderUnavailable)?
            .into_iter()
            .map(|r| ExchangeRate {
                currency_code: r.currency_code,
                rate: r.sats_per_unit,
                updated_at: r.updated_at,
            })
            .collect())
    }
}
