#![allow(dead_code)]

use crate::errors::Result;
use bitcoin::hashes::hex::ToHex;
use bitcoin::util::bip32::{DerivationPath, ExtendedPrivKey};
use bitcoin::Network;
use perro::MapToError;
use secp256k1::{PublicKey, SECP256K1};
use std::str::FromStr;

const PERSISTENCE_ENCRYPTION_KEY: &str = "m/76738065'/0'/1";

pub struct KeyPair {
    pub secret_key: [u8; 32],
    pub public_key: [u8; 33],
}

pub struct KeyPairHex {
    pub secret_key: String,
    pub public_key: String,
}

pub fn derive_persistence_encryption_key(seed: &[u8; 64]) -> Result<[u8; 32]> {
    Ok(derive_key_pair(seed, PERSISTENCE_ENCRYPTION_KEY)?.secret_key)
}

pub fn derive_key_pair_hex(seed: &[u8; 64], derivation_path: &str) -> Result<KeyPairHex> {
    let key_pair = derive_key_pair(seed, derivation_path)?;
    Ok(KeyPairHex {
        secret_key: key_pair.secret_key.to_vec().to_hex(),
        public_key: key_pair.public_key.to_hex(),
    })
}

pub fn derive_key_pair(seed: &[u8; 64], derivation_path: &str) -> Result<KeyPair> {
    let master_xpriv = ExtendedPrivKey::new_master(Network::Bitcoin, seed)
        .map_to_invalid_input("Failed to get xpriv from from seed")?;

    let derivation_path = DerivationPath::from_str(derivation_path)
        .map_to_invalid_input("Invalid derivation path")?;

    let derived_xpriv = master_xpriv
        .derive_priv(SECP256K1, &derivation_path)
        .map_to_permanent_failure("Failed to derive keys")?;

    let secret_key = derived_xpriv.private_key.secret_bytes();
    let public_key = PublicKey::from_secret_key(SECP256K1, &derived_xpriv.private_key).serialize();

    Ok(KeyPair {
        secret_key,
        public_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bip39::Mnemonic;
    use std::str::FromStr;

    const BACKEND_AUTH_DERIVATION_PATH: &str = "m/76738065'/0'/0";
    // Values used for testing were obtained from https://iancoleman.io/bip39
    const MNEMONIC_STR: &str = "between angry ketchup hill admit attitude echo wisdom still barrel coral obscure home museum trick grow magic eagle school tilt loop actress equal law";
    const SEED_HEX: &str = "781bfd3b2c6a5cfa9ed1551303fa20edf12baa5864521e7782d42a1bb15c2a444f7b81785f537bec6e38a533d0dc88e2a7effad7b975dd7c9bca1f9e7117966d";
    const DERIVED_AUTH_SECRET_KEY_HEX: &str =
        "1b64f7c3f7462e3815eacef53ddf18e5623bf8945d065761b05b022f19e60251";
    const DERIVED_AUTH_PUBLIC_KEY_HEX: &str =
        "02549b15801b155d32ca3931665361b1d2997ee531859b2d48cebbc2ccf21aac96";
    const DERIVED_ENCRYPTION_KEY_HEX: &str =
        "b51cda48891101f1e7b77e51e812da51d9c1b8b788d59e26e8af83d159f5a248";

    fn mnemonic_to_seed(mnemonic: &str) -> [u8; 64] {
        let mnemonic = Mnemonic::from_str(mnemonic).unwrap();
        let mut seed = [0u8; 64];
        seed.copy_from_slice(&mnemonic.to_seed("")[0..64]);

        seed
    }

    #[test]
    fn test_derive_auth_key_pair() {
        let seed = mnemonic_to_seed(MNEMONIC_STR);
        assert_eq!(seed.to_hex(), SEED_HEX.to_string());

        let key_pair = derive_key_pair_hex(&seed, BACKEND_AUTH_DERIVATION_PATH).unwrap();

        assert_eq!(key_pair.secret_key, DERIVED_AUTH_SECRET_KEY_HEX.to_string());
        assert_eq!(key_pair.public_key, DERIVED_AUTH_PUBLIC_KEY_HEX.to_string());
    }

    #[test]
    fn test_derive_persistence_encryption_key() {
        let seed = mnemonic_to_seed(MNEMONIC_STR);
        assert_eq!(seed.to_hex(), SEED_HEX.to_string());

        let encryption_key = derive_persistence_encryption_key(&seed).unwrap().to_hex();
        assert_eq!(encryption_key, DERIVED_ENCRYPTION_KEY_HEX.to_string());
    }
}
