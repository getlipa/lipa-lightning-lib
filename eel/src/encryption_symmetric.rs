#![allow(dead_code)]

use crate::errors::Result;
use crate::random;

use aes::Aes256;
use aes_gcm::{aead::Aead, AesGcm, Nonce};
use cipher::consts::U12;
use perro::MapToError;

pub(crate) fn encrypt(data: &[u8], key: &AesGcm<Aes256, U12>) -> Result<Vec<u8>> {
    let random_bytes = random::generate_random_bytes::<12>()?;
    let nonce: &Nonce<U12> = Nonce::from_slice(random_bytes.as_slice());

    let mut ciphertext = encrypt_vanilla(data, nonce, key)?;
    ciphertext.append(&mut random_bytes.to_vec());

    Ok(ciphertext)
}

pub(crate) fn decrypt(data: &[u8], key: &AesGcm<Aes256, U12>) -> Result<Vec<u8>> {
    let nonce_start = data.len() - 12;
    let nonce: &Nonce<U12> = Nonce::from_slice(&data[nonce_start..]);

    let plaintext = decrypt_vanilla(&data[..nonce_start], nonce, key)?;

    Ok(plaintext)
}

fn encrypt_vanilla(data: &[u8], nonce: &Nonce<U12>, key: &AesGcm<Aes256, U12>) -> Result<Vec<u8>> {
    key.encrypt(nonce, data)
        .map_to_permanent_failure("AES encryption failed")
}

fn decrypt_vanilla(data: &[u8], nonce: &Nonce<U12>, key: &AesGcm<Aes256, U12>) -> Result<Vec<u8>> {
    key.decrypt(nonce, data)
        .map_to_permanent_failure("AES decryption failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    use aes_gcm::Aes256Gcm;
    use cipher::KeyInit;

    const DUMMY_KEY: [u8; 32] = *b"A 32 byte long, non-random key.."; // 256 bits
    const DUMMY_NONCE: [u8; 12] = *b"mockup nonce"; // 96 bits
    const PLAINTEXT: [u8; 31] = *b"Not your keys, not your Bitcoin"; // size doesn't matter
    const CIPHERTEXT: [u8; 47] = [
        48, 10, 49, 121, 164, 149, 123, 109, 249, 136, 17, 88, 112, 5, 255, 244, 28, 37, 34, 91, 4,
        16, 10, 61, 154, 215, 95, 155, 208, 24, 204, 222, 98, 207, 64, 239, 40, 5, 198, 188, 161,
        28, 184, 155, 185, 99, 63,
    ];

    #[test]
    fn test_encryption() {
        let key = Aes256Gcm::new_from_slice(&DUMMY_KEY).unwrap();
        let nonce = Nonce::from_slice(&DUMMY_NONCE); // 96-bits; unique per message
        let plaintext = PLAINTEXT.to_vec();

        let ciphertext = encrypt_vanilla(&plaintext, &nonce, &key).unwrap();

        assert_eq!(ciphertext, CIPHERTEXT.to_vec());
    }

    #[test]
    fn test_decryption() {
        let key = Aes256Gcm::new_from_slice(&DUMMY_KEY).unwrap();
        let nonce = Nonce::from_slice(&DUMMY_NONCE); // 96-bits; unique per message
        let ciphertext = CIPHERTEXT.to_vec();

        let result = decrypt_vanilla(&ciphertext, &nonce, &key).unwrap();

        assert_eq!(result, PLAINTEXT.to_vec());
    }

    #[test]
    fn test_encryption_and_decryption_with_appended_random_nonce() {
        let key = Aes256Gcm::new_from_slice(&DUMMY_KEY).unwrap();

        let ciphertext = encrypt(&PLAINTEXT, &key).unwrap();
        assert_eq!(ciphertext.len(), CIPHERTEXT.len() + DUMMY_NONCE.len());
        assert_ne!(&ciphertext[..CIPHERTEXT.len()], &CIPHERTEXT);
        assert_ne!(&ciphertext[CIPHERTEXT.len()..], &DUMMY_NONCE);

        let result = decrypt(&ciphertext, &key).unwrap();
        assert_eq!(result, PLAINTEXT.to_vec());
    }
}
