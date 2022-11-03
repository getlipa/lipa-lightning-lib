use crate::errors::InitializationError;
use crate::secret::Secret;
use std::str::FromStr;

use bdk::keys::bip39::Mnemonic;
use lightning::chain::keysinterface::KeysManager;
use rand::rngs::OsRng;
use rand::RngCore;
use std::time::SystemTime;

pub fn init_keys_manager(seed: &Vec<u8>) -> Result<KeysManager, &str> {
    if seed.len() != 32 {
        return Err("Seed must have 32 bytes");
    }
    let mut array = [0; 32];
    array.copy_from_slice(&seed[0..32]);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| "System time before Unix epoch")?;
    Ok(KeysManager::new(&array, now.as_secs(), now.subsec_nanos()))
}

#[allow(clippy::result_large_err)]
pub fn generate_random_bytes() -> Result<[u8; 32], InitializationError> {
    let mut bytes = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| InitializationError::SecretGeneration {
            message: e.to_string(),
        })?;
    Ok(bytes)
}

#[allow(clippy::result_large_err)]
pub fn generate_secret(passphrase: String) -> Result<Secret, InitializationError> {
    let entropy = generate_random_bytes()?;
    let mnemonic =
        Mnemonic::from_entropy(&entropy).map_err(|e| InitializationError::SecretGeneration {
            message: e.to_string(),
        })?;
    let seed = mnemonic.to_seed(passphrase.clone())[0..32].to_vec();
    let mnemonic: Vec<String> = mnemonic.word_iter().map(|s| s.to_string()).collect();

    Ok(Secret {
        mnemonic,
        passphrase,
        seed,
    })
}

#[allow(clippy::result_large_err)]
pub fn mnemonic_to_secret(
    mnemonic_string: Vec<String>,
    passphrase: String,
) -> Result<Secret, InitializationError> {
    let mnemonic = Mnemonic::from_str(mnemonic_string.join(" ").as_str()).map_err(|e| {
        InitializationError::SecretGeneration {
            message: e.to_string(),
        }
    })?;

    let seed = mnemonic.to_seed(passphrase.clone())[0..32].to_vec();

    Ok(Secret {
        mnemonic: mnemonic_string,
        passphrase,
        seed,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_init() {
        assert!(init_keys_manager(&Vec::new()).is_err());

        let key = vec![0u8; 32];
        assert!(init_keys_manager(&key).is_ok());
    }

    #[test]
    fn test_secret_generation() {
        let secret = generate_secret("hodl".to_string()).unwrap();
        assert_eq!(secret.mnemonic.len(), 24);
        assert_eq!(secret.passphrase, "hodl");
        assert_eq!(secret.seed.len(), 32);
    }

    #[test]
    fn test_mnemonic_to_secret() {
        let secret = generate_secret("hodl".to_string()).unwrap();

        let secret_from_mnemonic =
            mnemonic_to_secret(secret.mnemonic.clone(), secret.passphrase.clone()).unwrap();

        assert_eq!(secret, secret_from_mnemonic);
    }
}
