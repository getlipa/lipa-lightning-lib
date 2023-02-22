use eel::errors::{Error, Result, RuntimeErrorCode};
use eel::interfaces::ExchangeRateProvider;
use honey_badger::Auth;
use std::sync::Arc;

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
    fn query_exchange_rate(&self, code: String) -> Result<u32> {
        self.provider
            .query_exchange_rate(code)
            .map_err(map_runtime_error)
    }
}

fn map_runtime_error<C: std::fmt::Display>(e: perro::Error<C>) -> Error {
    match e {
        perro::Error::InvalidInput { msg } => Error::InvalidInput { msg },
        perro::Error::RuntimeError { code, msg } => {
            let msg = format!("{code}: {msg}");
            Error::RuntimeError {
                code: RuntimeErrorCode::ExchangeRateProviderUnavailable,
                msg,
            }
        }
        perro::Error::PermanentFailure { msg } => Error::PermanentFailure { msg },
    }
}
