pub use graphql;

use crate::secrets::KeyPair;
use crate::{AuthLevel, TermsAndConditions};
pub use graphql::errors::{GraphQlRuntimeErrorCode, Result};

pub struct Auth {}

impl Auth {
    pub fn new(
        _backend_url: String,
        _auth_level: AuthLevel,
        _wallet_keypair: KeyPair,
        _auth_keypair: KeyPair,
    ) -> Result<Self> {
        Ok(Auth {})
    }

    pub async fn query_token(&self) -> Result<String> {
        Ok("dummy-token".to_string())
    }

    pub async fn get_wallet_pubkey_id(&self) -> Option<String> {
        Some("dummy-pubkey-id".to_string())
    }

    pub async fn accept_terms_and_conditions(
        &self,
        _terms: TermsAndConditions,
        _version: i64,
        _fingerprint: String,
    ) -> Result<()> {
        Ok(())
    }
}
