use crate::errors::Result;
use crate::random;
use crate::secret::Secret;

use bip39::{Language, Mnemonic};
use cipher::consts::U32;
use lightning::chain::keysinterface::KeysManager;
use perro::MapToError;
use std::str::FromStr;
use std::time::SystemTime;

pub(crate) fn init_keys_manager(seed: &[u8; 32]) -> Result<KeysManager> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_to_permanent_failure("System time before Unix epoch")?;
    Ok(KeysManager::new(seed, now.as_secs(), now.subsec_nanos()))
}

pub fn generate_secret(passphrase: String) -> Result<Secret> {
    let entropy = random::generate_random_bytes::<U32>()?;
    let mnemonic =
        Mnemonic::from_entropy(&entropy).map_to_permanent_failure("Failed to mnemonic")?;

    Ok(derive_secret_from_mnemonic(mnemonic, passphrase))
}

pub fn mnemonic_to_secret(mnemonic_string: Vec<String>, passphrase: String) -> Result<Secret> {
    let mnemonic =
        Mnemonic::from_str(&mnemonic_string.join(" ")).map_to_invalid_input("Invalid mnemonic")?;

    Ok(derive_secret_from_mnemonic(mnemonic, passphrase))
}

pub fn words_by_prefix(prefix: String) -> Vec<String> {
    Language::English
        .words_by_prefix(&prefix)
        .iter()
        .map(|w| w.to_string())
        .collect()
}

fn derive_secret_from_mnemonic(mnemonic: Mnemonic, passphrase: String) -> Secret {
    let seed = mnemonic.to_seed(&passphrase);
    let mnemonic_string: Vec<String> = mnemonic.word_iter().map(String::from).collect();
    Secret {
        mnemonic: mnemonic_string,
        passphrase,
        seed: seed.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::hex::FromHex;

    // The following constants were obtained from https://iancoleman.io/bip39/
    const MNEMONIC: &str = "main peasant found egg inner adapt kangaroo pretty woman amazing match depend visual deposit shrug about mule route much camera trash job glimpse light";
    const PASSPHRASE: &str = "hodl";
    const SEED: &str = "70f872e1a59e781e9c01f6328808baa774202f36281d7e74751b54693bc9c270b34c8c6d03e7c305189c84e15a641354ea79d7b76a9f062136be95ad4c1ae587";

    #[test]
    fn test_secret_generation() {
        let secret = generate_secret("hodl".to_string()).unwrap();
        assert_eq!(secret.mnemonic.len(), 24);
        assert_eq!(secret.passphrase, "hodl");
        assert_eq!(secret.seed.len(), 64);
    }

    #[test]
    fn test_mnemonic_to_secret() {
        let secret = generate_secret("hodl".to_string()).unwrap();

        let secret_from_mnemonic =
            mnemonic_to_secret(secret.mnemonic.clone(), secret.passphrase.clone()).unwrap();

        assert_eq!(secret, secret_from_mnemonic);
    }

    #[test]
    fn test_mnemonic_to_secret_hardcoded_values() {
        let mnemonic: Vec<String> = MNEMONIC.split_whitespace().map(String::from).collect();

        let passphrase = String::from(PASSPHRASE);

        let seed_expected = Vec::from_hex(SEED).unwrap();

        let secret = mnemonic_to_secret(mnemonic.clone(), passphrase.clone()).unwrap();

        assert_eq!(secret.mnemonic, mnemonic);
        assert_eq!(secret.passphrase, passphrase);
        assert_eq!(secret.seed, seed_expected);
    }

    #[test]
    fn test_words_by_prefix() {
        assert_eq!(words_by_prefix("".to_string()).len(), 2048);
        assert_eq!(words_by_prefix("s".to_string()).len(), 250);
        assert_eq!(words_by_prefix("sc".to_string()).len(), 15);
        assert_eq!(words_by_prefix("sch".to_string()), vec!["scheme", "school"]);
        assert_eq!(words_by_prefix("sche".to_string()), vec!["scheme"]);
        assert_eq!(words_by_prefix("scheme".to_string()), vec!["scheme"]);
        assert_eq!(words_by_prefix("schemelol".to_string()).len(), 0);
    }
}
