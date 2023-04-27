use eel::errors::{Result, RuntimeErrorCode};
use eel::interfaces::ExchangeRateProvider;
use honey_badger::Auth;
use perro::ResultTrait;
use std::sync::Arc;

pub struct ExchangeRateProviderImpl {
    provider: chameleon::ExchangeRateProvider,
}

impl ExchangeRateProviderImpl {
    pub fn new(graphql_url: String, auth: Arc<Auth>) -> Self {
        let provider = chameleon::ExchangeRateProvider::new(graphql_url, auth);
        Self { provider }
    }

    pub fn list_currency_codes(&self) -> Result<Vec<String>> {
        self.provider
            .list_currency_codes()
            .map_runtime_error_to(RuntimeErrorCode::ExchangeRateProviderUnavailable)
    }
}

impl ExchangeRateProvider for ExchangeRateProviderImpl {
    fn query_exchange_rate(&self, code: String) -> Result<u32> {
        self.provider
            .query_exchange_rate(code)
            .map_runtime_error_to(RuntimeErrorCode::ExchangeRateProviderUnavailable)
    }
}
