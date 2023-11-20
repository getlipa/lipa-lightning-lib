use crate::errors::Result;

use crate::key_derivation::derive_auth_keys;
use honey_badger::secrets::{generate_keypair, KeyPair};
use honey_badger::{Auth, AuthLevel};
use perro::MapToError;

pub(crate) fn build_auth(seed: &[u8; 64], graphql_url: String) -> Result<Auth> {
    let auth_keys = derive_auth_keys(seed)?;
    Auth::new(
        graphql_url,
        AuthLevel::Pseudonymous,
        auth_keys.into(),
        generate_keypair(),
    )
    .map_to_permanent_failure("Failed to build auth client")
}

pub(crate) fn build_async_auth(
    seed: &[u8; 64],
    graphql_url: String,
) -> Result<honey_badger::asynchronous::Auth> {
    let auth_keys = derive_auth_keys(seed)?;
    honey_badger::asynchronous::Auth::new(
        graphql_url,
        AuthLevel::Pseudonymous,
        auth_keys.into(),
        generate_keypair(),
    )
    .map_to_permanent_failure("Failed to build auth client")
}

impl From<crate::key_derivation::KeyPair> for KeyPair {
    fn from(value: crate::key_derivation::KeyPair) -> Self {
        Self {
            secret_key: hex::encode(value.secret_key),
            public_key: hex::encode(value.public_key),
        }
    }
}
