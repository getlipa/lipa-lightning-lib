use crate::errors::InitializationError;

use lightning::chain::keysinterface::KeysManager;
use rand::rngs::OsRng;
use rand::RngCore;
use std::time::SystemTime;

pub fn init_keys_manager(secret_seed: &Vec<u8>) -> Result<KeysManager, &str> {
    if secret_seed.len() != 32 {
        return Err("Secret seed must have 32 bytes");
    }
    let mut array = [0; 32];
    array.copy_from_slice(&secret_seed[0..32]);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| "SystemTime before UNIX EPOCH")?;
    Ok(KeysManager::new(&array, now.as_secs(), now.subsec_nanos()))
}

pub fn generate_secret_seed() -> Result<Vec<u8>, InitializationError> {
    let mut secret_seed = [0u8; 32];
    OsRng.try_fill_bytes(&mut secret_seed).map_err(|e| {
        InitializationError::SecretSeedGeneration {
            message: e.to_string(),
        }
    })?;
    Ok(secret_seed.to_vec())
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
    fn test_seed_generation() {
        let seed = generate_secret_seed().unwrap();
        assert_eq!(seed.len(), 32);
    }
}
