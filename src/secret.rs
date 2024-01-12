use crate::errors::{to_mnemonic_error, MnemonicError, SimpleError};
use crate::random::generate_random_bytes;

use bip39::{Language, Mnemonic};
use cipher::consts::U32;
use std::str::FromStr;

/// An object that holds necessary secrets. Should be dealt with carefully and never be logged.
///
/// The consumer of the library *must* persist `mnemonic` and `passphrase`
/// *securely* on the device,
/// The consumer of the library *must* never use or share it except to display it to
/// the end user for backup or for recovering a wallet.
/// The consumer of the library may want to *securely* persist `seed` or derive it
/// every time `seed` is needed, but it will have performance implications.
#[derive(PartialEq, Eq, Debug)]
pub struct Secret {
    /// The 24 words used to derive the node's private key.
    pub mnemonic: Vec<String>,
    /// Optional passphrase. If not provided, it is an empty string.
    pub passphrase: String,
    /// The seed derived from the mnemonic and the passphrase.
    pub seed: Vec<u8>,
}

impl Secret {
    fn from_mnemonic(mnemonic: Mnemonic, passphrase: String) -> Secret {
        let seed = mnemonic.to_seed(&passphrase);
        let mnemonic_string: Vec<String> = mnemonic.word_iter().map(String::from).collect();

        Secret {
            mnemonic: mnemonic_string,
            passphrase,
            seed: seed.to_vec(),
        }
    }
}

/// Generate a new mnemonic with an optional passphrase. Provide an empty string to use no passphrase.
// TODO LN-1658 requires mock implementation
pub fn generate_secret(passphrase: String) -> std::result::Result<Secret, SimpleError> {
    let entropy = generate_random_bytes::<U32>().map_err(|e| SimpleError::Simple {
        msg: format!("Failed to generate random bytes: {e}"),
    })?;
    let mnemonic = Mnemonic::from_entropy(&entropy).map_err(|e| SimpleError::Simple {
        msg: format!("Failed to generate mnemonic: {e}"),
    })?;

    Ok(Secret::from_mnemonic(mnemonic, passphrase))
}

/// Generate a Secret object (containing the seed). Provide an empty string to use no passphrase.
// TODO LN-1658 requires mock implementation
pub fn mnemonic_to_secret(
    mnemonic_string: Vec<String>,
    passphrase: String,
) -> std::result::Result<Secret, MnemonicError> {
    let mnemonic = Mnemonic::from_str(&mnemonic_string.join(" ")).map_err(to_mnemonic_error)?;
    Ok(Secret::from_mnemonic(mnemonic, passphrase))
}

/// Return a list of valid BIP-39 English words starting with the prefix.
/// Calling this function with empty prefix will return the full list of BIP-39 words.
// TODO LN-1658 requires mock implementation
pub fn words_by_prefix(prefix: String) -> Vec<String> {
    Language::English
        .words_by_prefix(&prefix)
        .iter()
        .map(|w| w.to_string())
        .collect()
}
