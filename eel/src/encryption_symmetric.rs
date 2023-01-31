#![allow(dead_code)]

use crate::errors::{Error, Result};
use crate::random;

use aes::Aes256;
use aes_gcm::Aes256Gcm;
use aes_gcm::{aead::Aead, AesGcm, Nonce};
use cipher::consts::U12;
use cipher::KeyInit;
use perro::MapToError;

pub(crate) fn encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let random_bytes = random::generate_random_bytes::<12>()?;
    let nonce: &Nonce<U12> = Nonce::from_slice(random_bytes.as_slice());

    let mut ciphertext = encrypt_vanilla(data, key, nonce)?;
    ciphertext.append(&mut random_bytes.to_vec());

    Ok(ciphertext)
}

pub(crate) fn decrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    if data.len() <= 12 {
        return Err(Error::InvalidInput {
            msg: format!(
                "Ciphertext is only {} bytes long, but appended nonce alone must be 12 bytes.",
                data.len()
            ),
        });
    }

    let nonce_start = data.len() - 12;
    let nonce: &Nonce<U12> = Nonce::from_slice(&data[nonce_start..]);

    decrypt_vanilla(&data[..nonce_start], key, nonce)
}

fn encrypt_vanilla(data: &[u8], key: &[u8], nonce: &Nonce<U12>) -> Result<Vec<u8>> {
    let key = bytes_to_key(key)?;

    key.encrypt(nonce, data)
        .map_to_invalid_input("AES encryption failed")
}

fn decrypt_vanilla(data: &[u8], key: &[u8], nonce: &Nonce<U12>) -> Result<Vec<u8>> {
    let key = bytes_to_key(key)?;

    key.decrypt(nonce, data)
        .map_to_invalid_input("AES decryption failed")
}

fn bytes_to_key(data: &[u8]) -> Result<AesGcm<Aes256, U12>> {
    Aes256Gcm::new_from_slice(data).map_to_invalid_input("Invalid AES key")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_KEY: [u8; 32] = *b"A 32 byte long, non-random key.."; // 256 bits
    const UNSECURE_KEY: [u8; 9] = *b"short key"; // 72 bits
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

        let ciphertext = encrypt_vanilla(&plaintext, &DUMMY_KEY, &nonce).unwrap();

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

    #[test]
    fn use_of_unsecure_key_forbidden() {
        let result = encrypt(&PLAINTEXT, &UNSECURE_KEY);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidInput { .. }));
    }
}
