pub mod asynchronous;
pub mod secrets;

use crate::secrets::KeyPair;
pub use graphql::errors::{GraphQlRuntimeErrorCode, Result};
pub use honeybadger::{AuthLevel, TermsAndConditions};
use std::time::SystemTime;
pub struct Auth {}

#[derive(Debug, PartialEq)]
pub struct TermsAndConditionsStatus {
    pub accepted_at: Option<SystemTime>,
    pub terms_and_conditions: TermsAndConditions,
    pub version: i64,
}

impl Auth {
    pub fn new(
        _backend_url: String,
        _auth_level: AuthLevel,
        _wallet_keypair: KeyPair,
        _auth_keypair: KeyPair,
    ) -> Result<Self> {
        Ok(Auth {})
    }

    pub fn query_token(&self) -> Result<String> {
        Ok("dummy-token".to_string())
    }

    pub fn get_wallet_pubkey_id(&self) -> Result<String> {
        Ok("dummy-pubkey-id".to_string())
    }

    pub fn accept_terms_and_conditions(
        &self,
        _terms: TermsAndConditions,
        _version: i64,
    ) -> Result<()> {
        Ok(())
    }

    pub fn get_terms_and_conditions_status(
        &self,
        terms: TermsAndConditions,
    ) -> Result<TermsAndConditionsStatus> {
        Ok(TermsAndConditionsStatus {
            accepted_at: Some(SystemTime::now()),
            terms_and_conditions: terms,
            version: 1,
        })
    }
}
