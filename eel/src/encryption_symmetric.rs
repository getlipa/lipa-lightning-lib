use crate::errors::{Error, Result};
use crate::random;

use aes_gcm::Aes256Gcm;
use aes_gcm::{aead::Aead, Nonce as AesNonce};
use cipher::consts::U12;
use cipher::{KeyInit, Unsigned};
use perro::MapToError;

type NonceLength = U12;
type Nonce = AesNonce<NonceLength>;

pub(crate) fn encrypt(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let nonce = random::generate_random_bytes::<NonceLength>()?;

    let mut ciphertext = encrypt_vanilla(data, key, &nonce)?;
    ciphertext.extend_from_slice(nonce.as_ref());

    Ok(ciphertext)
}

pub(crate) fn decrypt(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    if data.len() <= NonceLength::USIZE {
        return Err(Error::InvalidInput {
            msg: format!(
                "Ciphertext is only {} bytes long, but appended nonce alone must be {} bytes.",
                data.len(),
                NonceLength::USIZE,
            ),
        });
    }

    let nonce_start = data.len() - NonceLength::USIZE;
    let (data, nonce) = data.split_at(nonce_start);
    let nonce = Nonce::from_slice(nonce);

    decrypt_vanilla(data, key, nonce)
}

fn encrypt_vanilla(data: &[u8], key: &[u8; 32], nonce: &Nonce) -> Result<Vec<u8>> {
    let cipher = make_cipher(key)?;
    cipher
        .encrypt(nonce, data)
        .map_to_permanent_failure("AES encryption failed")
}

fn decrypt_vanilla(data: &[u8], key: &[u8; 32], nonce: &Nonce) -> Result<Vec<u8>> {
    let cipher = make_cipher(key)?;
    cipher
        .decrypt(nonce, data)
        .map_to_invalid_input("AES decryption failed")
}

fn make_cipher(key: &[u8; 32]) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_to_invalid_input("Invalid AES key")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_KEY: [u8; 32] = *b"A 32 byte long, non-random key.."; // 256 bits
    const DUMMY_NONCE: [u8; 12] = *b"mockup nonce"; // 96 bits
    const PLAINTEXT: [u8; 31] = *b"Not your keys, not your Bitcoin"; // size doesn't matter
    const CIPHERTEXT: [u8; 47] = [
        48, 10, 49, 121, 164, 149, 123, 109, 249, 136, 17, 88, 112, 5, 255, 244, 28, 37, 34, 91, 4,
        16, 10, 61, 154, 215, 95, 155, 208, 24, 204, 222, 98, 207, 64, 239, 40, 5, 198, 188, 161,
        28, 184, 155, 185, 99, 63,
    ];
    const FLAWED_CIPHERTEXT: [u8; 3] = [48, 10, 49];

    #[test]
    fn test_encryption() {
        let nonce = Nonce::from_slice(&DUMMY_NONCE); // 96-bits; unique per message
        let plaintext = PLAINTEXT.to_vec();

        let ciphertext = encrypt_vanilla(&plaintext, &DUMMY_KEY, nonce).unwrap();

        assert_eq!(ciphertext, CIPHERTEXT.to_vec());
    }

    #[test]
    fn test_decryption() {
        let nonce = Nonce::from_slice(&DUMMY_NONCE); // 96-bits; unique per message
        let ciphertext = CIPHERTEXT.to_vec();

        let result = decrypt_vanilla(&ciphertext, &DUMMY_KEY, nonce).unwrap();

        assert_eq!(result, PLAINTEXT.to_vec());
    }

    #[test]
    fn test_encryption_and_decryption_with_appended_random_nonce() {
        let ciphertext = encrypt(&PLAINTEXT, &DUMMY_KEY).unwrap();
        assert_eq!(ciphertext.len(), CIPHERTEXT.len() + DUMMY_NONCE.len());
        assert_ne!(&ciphertext[..CIPHERTEXT.len()], &CIPHERTEXT);
        assert_ne!(&ciphertext[CIPHERTEXT.len()..], &DUMMY_NONCE);

        let result = decrypt(&ciphertext, &DUMMY_KEY).unwrap();
        assert_eq!(result, PLAINTEXT.to_vec());
    }

    #[test]
    fn test_flawed_decryption() {
        let result = decrypt(&FLAWED_CIPHERTEXT, &DUMMY_KEY);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidInput { .. }));
    }
}
