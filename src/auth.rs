use honey_badger::AuthLevel;
use std::sync::Arc;

use eel::errors::Result;
use eel::key_derivation::derive_key_pair_hex;
use eel::MapToError;

// Backwards-compatibility / Technical debt mess
// Be ready for a little story telling...
// Apparently, the BACKEND_WALLET_DERIVATION_PATH should be the actual key used for authorization
// However, because they used a the derivation math "m" (master key) for that in other services
// they figured it would be insecure to keep this key in memory all the time (which is true),
// just for authorizing oneself towards the backend.
// So their solution was to sign a secondary, somewhat ephemeral key pair with the master key,
// and then use that key pair for authorization (here called BACKEND_AUTH_DERIVATION_PATH).
// Since this code right here is using a sensible key derivation path from the get-go,
// this scheme would not be necessary. However, the backend still expects the old scheme,
// so that's why we just derive 2 key pairs, and follow the overhead of the old scheme.
// For the ones who have access, follow this issue: https://getlipa.atlassian.net/browse/APP-1057
const BACKEND_WALLET_DERIVATION_PATH: &str = "m/76738065'/0'/0";
const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/2";

pub struct Auth {
    auth: Arc<honey_badger::Auth>,
}

impl Auth {
    pub fn new(backend_url: String, seed: &[u8; 64]) -> Result<Self> {
        let wallet_keypair = derive_key_pair_hex(seed, BACKEND_WALLET_DERIVATION_PATH)
            .map_to_permanent_failure("Could not derive wallet keypair")?;
        let auth_keypair = derive_key_pair_hex(seed, BACKEND_AUTH_DERIVATION_PATH)
            .map_to_permanent_failure("Could not derive auth keypair")?;
        let wallet_keypair = honey_badger::secrets::KeyPair {
            secret_key: wallet_keypair.secret_key,
            public_key: wallet_keypair.public_key,
        };
        let auth_keypair = honey_badger::secrets::KeyPair {
            secret_key: auth_keypair.secret_key,
            public_key: auth_keypair.public_key,
        };

        Ok(Auth {
            auth: Arc::new(
                honey_badger::Auth::new(
                    backend_url,
                    AuthLevel::Pseudonymous,
                    wallet_keypair,
                    auth_keypair,
                )
                .map_to_permanent_failure("Could not initiate auth object")?,
            ),
        })
    }

    pub fn get_instace(&self) -> Arc<honey_badger::Auth> {
        Arc::clone(&self.auth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    const DUMMY_SEED: [u8; 64] = [0; 64];

    #[test]
    fn test_auth_flow() {
        let auth = Auth::new(get_backend_url(), &DUMMY_SEED).unwrap();
        assert!(auth.auth.query_token().unwrap().starts_with("ey")); // JWT
    }

    fn get_backend_url() -> String {
        format!(
            "{}/v1/graphql",
            env::var("BACKEND_BASE_URL").expect("BACKEND_BASE_URL environment variable is not set")
        )
    }
}
