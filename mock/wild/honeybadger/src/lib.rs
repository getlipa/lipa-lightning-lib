pub mod asynchronous;
pub mod secrets;

use crate::secrets::KeyPair;
pub use graphql::errors::{GraphQlRuntimeErrorCode, Result};
pub use honeybadger::AuthLevel;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

lazy_static! {
    static ref TAC_STORE: Mutex<HashMap<TermsAndConditions, TermsAndConditionsStatus>> =
        Mutex::new(HashMap::new());
}

pub struct Auth {}

// Redefining instead of importing because we need Eq and Hash for the Hashmap (automatically deduplicate)
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub enum TermsAndConditions {
    Lipa,
    Pocket,
}

// Redefining instead of importing because we need Clone
#[derive(Debug, PartialEq, Clone)]
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
        terms: TermsAndConditions,
        version: i64,
        _fingerprint: String,
    ) -> Result<()> {
        TAC_STORE.lock().unwrap().insert(
            terms.clone(),
            TermsAndConditionsStatus {
                accepted_at: Some(SystemTime::now()),
                terms_and_conditions: terms,
                version,
            },
        );

        Ok(())
    }

    pub fn get_terms_and_conditions_status(
        &self,
        terms: TermsAndConditions,
    ) -> Result<TermsAndConditionsStatus> {
        match TAC_STORE.lock().unwrap().get(&terms) {
            Some(status) => Ok(status.clone()),
            None => Ok(TermsAndConditionsStatus {
                accepted_at: None,
                terms_and_conditions: terms.clone(),
                version: 0,
            }),
        }
    }
}
