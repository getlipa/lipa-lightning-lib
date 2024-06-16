use aes_gcm::{Aes256Gcm, KeyInit};
use secp256k1::hashes::{sha256, Hash, HashEngine, Hmac, HmacEngine};
use secp256k1::PublicKey;

#[derive(Debug)]
pub struct RedeemerKey {
    pub(crate) key: PublicKey,
}

impl RedeemerKey {
    pub(crate) fn new(key: PublicKey) -> Self {
        Self { key }
    }

    pub(crate) fn derive_symmetric_key(&self) -> Aes256Gcm {
        let key = self.key.x_only_public_key().0.serialize();
        let mut engine = HmacEngine::<sha256::Hash>::new(&key);
        engine.input(b"lightning voucher");
        let hmac = Hmac::<sha256::Hash>::from_engine(engine);
        Aes256Gcm::new(hmac.as_byte_array().into())
    }

    pub fn encode(&self) -> String {
        data_encoding::BASE64URL_NOPAD.encode(&self.key.serialize())
    }

    pub fn decode(key: &str) -> Self {
        let key = data_encoding::BASE64URL_NOPAD
            .decode(key.as_bytes())
            .unwrap();
        let key = PublicKey::from_slice(&key).unwrap();
        Self { key }
    }

    pub fn hash(&self) -> String {
        sha256::Hash::hash(&self.key.serialize()).to_string()
    }
}
