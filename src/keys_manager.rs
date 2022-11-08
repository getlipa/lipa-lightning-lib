use crate::errors::*;
use crate::secret::Secret;

use bdk::keys::bip39::Mnemonic;
use lightning::chain::keysinterface::KeysManager;
use rand::rngs::OsRng;
use rand::RngCore;
use std::str::FromStr;
use std::time::SystemTime;

pub(crate) fn init_keys_manager(seed: &Vec<u8>) -> LipaResult<KeysManager> {
    if seed.len() != 32 {
        return Err(invalid_input("Seed must have 32 bytes"));
    }
    let mut array = [0; 32];
    array.copy_from_slice(&seed[0..32]);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_to_permanent_failure("System time before Unix epoch")?;
    Ok(KeysManager::new(&array, now.as_secs(), now.subsec_nanos()))
}

pub(crate) fn generate_random_bytes<const N: usize>() -> LipaResult<[u8; N]> {
    let mut bytes = [0u8; N];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_to_permanent_failure("Failed to generate random bytes")?;
    Ok(bytes)
}

pub fn generate_secret(passphrase: String) -> LipaResult<Secret> {
    let entropy = generate_random_bytes::<32>()?;
    let mnemonic =
        Mnemonic::from_entropy(&entropy).map_to_permanent_failure("Failed to mnemonic")?;
    let mnemonic_string: Vec<String> = mnemonic.word_iter().map(|s| s.to_string()).collect();

    Ok(derive_secret_from_mnemonic(
        mnemonic,
        mnemonic_string,
        passphrase,
    ))
}

pub fn mnemonic_to_secret(mnemonic_string: Vec<String>, passphrase: String) -> LipaResult<Secret> {
    let mnemonic =
        Mnemonic::from_str(&mnemonic_string.join(" ")).map_to_invalid_input("Invalid mnemonic")?;

    Ok(derive_secret_from_mnemonic(
        mnemonic,
        mnemonic_string,
        passphrase,
    ))
}

fn derive_secret_from_mnemonic(
    mnemonic: Mnemonic,
    mnemonic_string: Vec<String>,
    passphrase: String,
) -> Secret {
    let seed = mnemonic.to_seed(&passphrase)[0..32].to_vec();

    Secret {
        mnemonic: mnemonic_string,
        passphrase,
        seed,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bitcoin::hashes::hex::FromHex;

    // The following constants were obtained from https://iancoleman.io/bip39/
    const MNEMONIC: &str = "main peasant found egg inner adapt kangaroo pretty woman amazing match depend visual deposit shrug about mule route much camera trash job glimpse light";
    const PASSPHRASE: &str = "hodl";
    const SEED: &str = "70f872e1a59e781e9c01f6328808baa774202f36281d7e74751b54693bc9c270";

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

    #[test]
    fn test_mnemonic_to_secret_hardcoded_values() {
        let mnemonic: Vec<String> = MNEMONIC.split_whitespace().map(|s| s.to_string()).collect();

        let passphrase = String::from(PASSPHRASE);

        let seed_expected = Vec::from_hex(SEED).unwrap();

        let secret = mnemonic_to_secret(mnemonic.clone(), passphrase.clone()).unwrap();

        assert_eq!(secret.mnemonic, mnemonic);
        assert_eq!(secret.passphrase, passphrase);
        assert_eq!(secret.seed, seed_expected);
    }
}
