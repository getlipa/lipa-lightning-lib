use aes_gcm::aead::Aead;
use aes_gcm::{AeadCore, Aes256Gcm, KeyInit, Nonce};
use cipher::consts::U32;
use cipher::Unsigned;
use rand::rngs::OsRng;
use secp256k1::{generate_keypair, PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};

type U32Bytes = generic_array::GenericArray<u8, U32>;

pub fn key_to_hash(key: &PublicKey) -> String {
    let hash = sha256(&key.serialize());
    data_encoding::HEXLOWER.encode(&hash)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VoucherMetadata {
    pub amount_range_sat: (u32, u32),
    pub description: String,
    pub issued_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Debug)]
pub struct Voucher {
    pub redeemer_key: PublicKey,
    pub metadata: VoucherMetadata,
}

#[derive(Debug)]
pub struct EncryptedVoucher {
    pub hash: String,
    pub metadata: Vec<u8>,
}

impl Voucher {
    pub fn generate(
        amount_range_sat: (u32, u32),
        description: String,
        expires_in: Duration,
    ) -> (Self, SecretKey) {
        let (issuer_key, redeemer_key) = generate_keypair(&mut rand::thread_rng());

        if amount_range_sat.0 > amount_range_sat.1 {}
        let issued_at = SystemTime::now();
        let metadata = VoucherMetadata {
            amount_range_sat,
            description,
            issued_at,
            expires_at: issued_at.checked_add(expires_in).unwrap(),
        };
        let voucher = Self {
            redeemer_key,
            metadata,
        };
        (voucher, issuer_key)
    }

    pub fn encrypt(&self) -> EncryptedVoucher {
        let cipher =
            Aes256Gcm::new_from_slice(&self.redeemer_key.x_only_public_key().0.serialize())
                .unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let metadata = serde_json::to_string(&self.metadata).unwrap();
        let mut metadata = cipher.encrypt(&nonce, metadata.as_bytes()).unwrap();
        metadata.extend_from_slice(&nonce);

        let hash = key_to_hash(&self.redeemer_key);
        EncryptedVoucher { hash, metadata }
    }

    pub fn decrypt(redeemer_key: PublicKey, metadata: &[u8]) -> Self {
        const NONCE_SIZE: usize = <Aes256Gcm as AeadCore>::NonceSize::USIZE;
        if metadata.len() < NONCE_SIZE {}

        let nonce_start = metadata.len() - NONCE_SIZE;
        let (metadata, nonce) = metadata.split_at(nonce_start);
        let nonce = Nonce::from_slice(nonce);
        let cipher = Aes256Gcm::new(&symmetric_key(&redeemer_key));
        let metadata = cipher.decrypt(nonce, metadata).unwrap();
        let metadata = String::from_utf8(metadata).unwrap();
        let metadata: VoucherMetadata = serde_json::from_str(&metadata).unwrap();

        Self {
            redeemer_key,
            metadata,
        }
    }
}

fn symmetric_key(key: &PublicKey) -> U32Bytes {
    key.x_only_public_key().0.serialize().into()
}

fn sha256(data: &[u8]) -> U32Bytes {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lnurl::*;

    #[test]
    fn it_works() {
        // Client.
        let (voucher, _issuer_key) =
            Voucher::generate((10, 12), "Descr".to_string(), Duration::from_secs(60));
        println!("{voucher:?}");
        // Store voucher to the local db.
        let encrypted = voucher.encrypt();
        println!("{encrypted:?}");
        // Register encrypted voucher on the server.
        let lnurl_prefix = "https://zzd.es/lnurl/";
        let lnurl = encode(&voucher, lnurl_prefix);
        println!("{lnurl}");
        // Send lnurl to the recipient.

        // Server.
        let key = decode(&lnurl);
        let hash = key_to_hash(&key);
        println!("{hash}");
        // Look up encrypted metadata by hash.
        let v = Voucher::decrypt(key, &encrypted.metadata);
        println!("{v:?}");
        let r = to_lnurl_response(&v, lnurl_prefix);
        println!("{r}");
    }
}
