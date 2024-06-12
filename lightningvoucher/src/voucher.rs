use aes_gcm::aead::Aead;
use aes_gcm::{AeadCore, Aes256Gcm, KeyInit, Nonce};
use cipher::consts::U32;
use cipher::Unsigned;
use rand::rngs::OsRng;
use secp256k1::ecdsa::Signature;
use secp256k1::hashes::{sha256, Hash};
use secp256k1::{generate_keypair, Message, PublicKey, SecretKey, SECP256K1};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::{Duration, SystemTime};

type U32Bytes = generic_array::GenericArray<u8, U32>;

pub fn key_to_hash(key: &PublicKey) -> String {
    sha256::Hash::hash(&key.serialize()).to_string()
}

#[derive(Debug)]
pub struct VoucherMetadata {
    pub amount_range_sat: (u64, u64),
    pub description: String,
    pub issued_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoucherMetadataJson {
    min_withdrawable: u64,
    max_withdrawable: u64,
    default_description: String,
    expires_at: u64,
}

impl From<&VoucherMetadata> for VoucherMetadataJson {
    fn from(m: &VoucherMetadata) -> Self {
        let expires_at = m
            .expires_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            min_withdrawable: m.amount_range_sat.0 * 1000,
            max_withdrawable: m.amount_range_sat.1 * 1000,
            default_description: m.description.clone(),
            expires_at,
        }
    }
}

impl From<VoucherMetadataJson> for VoucherMetadata {
    fn from(v: VoucherMetadataJson) -> Self {
        let min = v.min_withdrawable / 1000;
        let max = v.max_withdrawable / 1000;
        // TODO: Do better.
        let issued_at = SystemTime::now();
        let expires_at = SystemTime::now();
        VoucherMetadata {
            amount_range_sat: (min, max),
            description: v.default_description,
            issued_at,
            expires_at,
        }
    }
}

impl Serialize for VoucherMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        VoucherMetadataJson::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VoucherMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        VoucherMetadataJson::deserialize(deserializer).map(VoucherMetadataJson::into)
    }
}

#[derive(Debug)]
pub struct Voucher {
    pub redeemer_key: PublicKey,
    pub metadata: VoucherMetadata,
    pub signature: Signature,
}

#[derive(Debug)]
pub struct EncryptedVoucher {
    pub hash: String,
    pub data: Vec<u8>,
}

impl Voucher {
    pub fn generate(
        amount_range_sat: (u64, u64),
        description: String,
        expires_in: Duration,
    ) -> (Self, SecretKey) {
        let (issuer_key, redeemer_key) = generate_keypair(&mut rand::thread_rng());

        if amount_range_sat.0 > amount_range_sat.1 {
            panic!("min should be less or equal than max");
        }
        let issued_at = SystemTime::now();
        let metadata = VoucherMetadata {
            amount_range_sat,
            description,
            issued_at,
            expires_at: issued_at.checked_add(expires_in).unwrap(),
        };

        let metadata_json = serde_json::to_string(&metadata).unwrap();
        let message =
            Message::from_hashed_data::<secp256k1::hashes::sha256::Hash>(metadata_json.as_bytes());
        let signature = SECP256K1.sign_ecdsa(&message, &issuer_key);

        let voucher = Self {
            redeemer_key,
            metadata,
            signature,
        };
        (voucher, issuer_key)
    }

    pub fn encrypt(&self) -> EncryptedVoucher {
        let hash = key_to_hash(&self.redeemer_key);

        let cipher = Aes256Gcm::new(&symmetric_key(&self.redeemer_key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let metadata = serde_json::to_string(&self.metadata).unwrap();
        let mut data = cipher.encrypt(&nonce, metadata.as_bytes()).unwrap();
        data.extend_from_slice(&nonce);
        data.extend_from_slice(&self.signature.serialize_compact());

        EncryptedVoucher { hash, data }
    }

    pub fn decrypt(redeemer_key: PublicKey, data: &[u8]) -> Self {
        const NONCE_SIZE: usize = <Aes256Gcm as AeadCore>::NonceSize::USIZE;
        if data.len() < NONCE_SIZE {
            panic!("data is too short");
        }

        let signature_start = data.len() - 64;
        let (data, signature) = data.split_at(signature_start);

        let nonce_start = data.len() - NONCE_SIZE;
        let (metadata, nonce) = data.split_at(nonce_start);

        let nonce = Nonce::from_slice(nonce);
        let cipher = Aes256Gcm::new(&symmetric_key(&redeemer_key));
        let metadata = cipher.decrypt(nonce, metadata).unwrap();
        let metadata = String::from_utf8(metadata).unwrap();
        let metadata: VoucherMetadata = serde_json::from_str(&metadata).unwrap();

        let signature = Signature::from_compact(signature).unwrap();
        // TODO: Verify signature.

        Self {
            redeemer_key,
            metadata,
            signature,
        }
    }

    // TODO: Add methods for redeemer.
}

fn symmetric_key(key: &PublicKey) -> U32Bytes {
    // TODO: Hash with something to get "random" bytes.
    key.x_only_public_key().0.serialize().into()
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
        println!("{}", encrypted.hash);
        // Register encrypted voucher on the server.
        let lnurl_prefix = "https://zzd.es/lnurl/";
        let lnurl = encode(&voucher, lnurl_prefix);
        println!("{lnurl}");
        // Send lnurl to the recipient.

        // Server.
        let redeemer_key = decode(&lnurl);
        let hash = key_to_hash(&redeemer_key);
        println!("{hash}");
        // Look up encrypted metadata by hash.
        let v = Voucher::decrypt(redeemer_key, &encrypted.data);
        println!("{v:?}");
        let r = to_lnurl_response(&v, lnurl_prefix.to_string());
        println!("{r}");
    }
}
