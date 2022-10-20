use crate::errors::RuntimeError;

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, BlockSizeUser, KeyIvInit};
use bitcoin::hashes::{sha256, sha512, Hash, HashEngine, Hmac, HmacEngine};
use bitcoin::secp256k1::rand::thread_rng;
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use rand::rngs::OsRng;
use rand::RngCore;

const CIPH_CURVE_BYTES: [u8; 2] = [0x02, 0xCA]; // 0x02CA = 714
const CIPH_COORD_LEN: [u8; 2] = [0x00, 0x20]; // 0x20 = 32

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

// Implements Encrypt() from btcsuite/btcd
// https://pkg.go.dev/github.com/btcsuite/btcd/btcec#Encrypt
// https://github.com/btcsuite/btcd/blob/v0.22.1/btcec/ciphering.go#L70
#[allow(dead_code)]
pub(crate) fn encrypt(pubkey: PublicKey, data: &[u8]) -> Result<Vec<u8>, RuntimeError> {
    let secp = Secp256k1::new();
    let (ephemeral, ephemeral_pubkey) = secp.generate_keypair(&mut thread_rng());
    let mut init_vector = vec![0u8; Aes256CbcEnc::block_size()];
    OsRng.fill_bytes(&mut init_vector);
    let randomness = Randomness {
        ephemeral,
        ephemeral_pubkey,
        init_vector,
    };
    encrypt_with_randomness(pubkey, data, &randomness)
}

struct Randomness {
    ephemeral: SecretKey,
    ephemeral_pubkey: PublicKey,
    init_vector: Vec<u8>,
}

fn encrypt_with_randomness(
    pubkey: PublicKey,
    data: &[u8],
    randomness: &Randomness,
) -> Result<Vec<u8>, RuntimeError> {
    let shared_secret = generate_shared_secret(&randomness.ephemeral, pubkey)?;
    let key_e = &shared_secret[..32];
    let key_m = &shared_secret[32..];

    // IV + Curve params/X/Y + padded ciphertext + HMAC-256
    let mut result = Vec::new();
    result.extend_from_slice(&randomness.init_vector);

    let ephemeral_pubkey = randomness.ephemeral_pubkey.serialize_uncompressed();
    result.extend_from_slice(&CIPH_CURVE_BYTES);
    result.extend_from_slice(&CIPH_COORD_LEN);
    result.extend_from_slice(&ephemeral_pubkey[1..33]);
    result.extend_from_slice(&CIPH_COORD_LEN);
    result.extend_from_slice(&ephemeral_pubkey[33..]);

    let cipher = Aes256CbcEnc::new_from_slices(key_e, &randomness.init_vector).map_err(|_| {
        RuntimeError::Logic {
            message: "Invalid key or nonce lenght in encrypt()".to_string(),
        }
    })?;
    let mut ciphertext = cipher.encrypt_padded_vec_mut::<Pkcs7>(data);
    result.append(&mut ciphertext);

    let mut hmac = hmac256(key_m, &result);
    result.append(&mut hmac);

    Ok(result)
}

fn generate_shared_secret(
    privkey: &SecretKey,
    mut pubkey: PublicKey,
) -> Result<[u8; 64], RuntimeError> {
    // Unfortunately we cannot use secp256k1::ecdh::SharedSecret, because it uses
    // sha256, but we need sha512.

    let secp = Secp256k1::new();
    let scalar = privkey.secret_bytes();
    pubkey
        .mul_assign(&secp, &scalar)
        .map_err(|_| RuntimeError::Logic {
            message: "Multiplication should never fail with a verified seckey and valid pubkey"
                .to_string(),
        })?;
    // https://github.com/bitcoin-core/secp256k1/blob/master/src/eckey_impl.h#L43
    let x_coordinate = &pubkey.serialize()[1..33];
    sha512::Hash::hash(x_coordinate)[..]
        .try_into()
        .map_err(|_| RuntimeError::Logic {
            message: "Sha512 returns less than 64 bytes".to_string(),
        })
}

fn hmac256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut engine = HmacEngine::<sha256::Hash>::new(key);
    engine.input(data);
    Hmac::<sha256::Hash>::from_engine(engine)[..].to_vec()
}

#[cfg(test)]
mod test {
    use super::*;
    use bitcoin_hashes::hex::FromHex;
    use bitcoin_hashes::hex::ToHex;

    #[test]
    fn test_generate_shared_secret() {
        let secp = Secp256k1::new();
        let ephemeral = bitcoin::secp256k1::ONE_KEY;
        let ephemeral_pubkey = PublicKey::from_secret_key(&secp, &ephemeral);
        let privkey =
            Vec::from_hex("6afa9046a9579cad143a384c1b564b9a250d27d6f6a63f9f20bf3a7594c9e2c6")
                .unwrap();
        let privkey = SecretKey::from_slice(&privkey).unwrap();
        let pubkey = PublicKey::from_secret_key(&secp, &privkey);
        let init_vector = Vec::from_hex("6afa9046a9579cad143a384c1b564b9a").unwrap();
        let randomness = Randomness {
            ephemeral,
            ephemeral_pubkey,
            init_vector,
        };
        let shared_secret = generate_shared_secret(&randomness.ephemeral, pubkey)
            .unwrap()
            .to_hex();
        assert_eq!(
            shared_secret,
            "2e46538a92f7f39569abeab41128e298271102d17ad9108262b9a2f044a86acd\
	     ffee438e1bfc9f796333f32b50231edbd78d6906bcb17a4d7504e39da6e17e78"
        );
    }

    fn hmac(key: &str, data: &str) -> String {
        hmac256(key.as_bytes(), data.as_bytes()).to_hex()
    }

    #[test]
    pub fn test_hmac256() {
        // From https://www.devglan.com/online-tools/hmac-sha256-online
        assert_eq!(
            hmac("key", "text"),
            "6afa9046a9579cad143a384c1b564b9a250d27d6f6a63f9f20bf3a7594c9e2c6"
        );
        assert_eq!(
            hmac(
                "key",
                "longlonglonglonglonglonglonglonglonglonglonglonglonglonglonglong\
		 longlonglonglonglonglonglonglonglonglonglonglonglonglonglonglong\
		 longlonglonglonglonglonglonglonglonglonglonglonglong"
            ),
            "693d55a098b1adccc318cff89514875c82b20198e44bcc45fdb560e1272c988d"
        );
        assert_eq!(
            hmac(
                "longkeylongkeylongkeylongkeylongkeylongkeylongkeylongkeylongkeyl\
		  ongkeylongkeylongkeylongkeylongkeylongkeylongkeylongkeylongkeylo\
		  ngkeylongkeylongkeylongkeylongkeylongkeylongkey",
                "longlonglonglonglonglonglonglonglonglonglonglonglonglonglonglongl\
		  onglonglonglonglonglonglonglonglonglonglonglonglonglonglonglonglo\
		  nglonglonglonglonglonglonglonglonglonglonglonglong"
            ),
            "55b061f0a90aa23992c3d1e12348ab656a8724d32bc6e55de881146723c64f0e"
        );
    }

    #[test]
    pub fn test_encrypt() {
        // Tested against Decrypt() from btcsuite/btcd
        // https://github.com/btcsuite/btcd/blob/v0.22/btcec/ciphering.go#L121
        let secp = Secp256k1::new();
        let ephemeral = bitcoin::secp256k1::ONE_KEY;
        let ephemeral_pubkey = PublicKey::from_secret_key(&secp, &ephemeral);
        let init_vector = Vec::from_hex("6afa9046a9579cad143a384c1b564b9a").unwrap();
        let randomness = Randomness {
            ephemeral,
            ephemeral_pubkey,
            init_vector,
        };
        let privkey =
            Vec::from_hex("6afa9046a9579cad143a384c1b564b9a250d27d6f6a63f9f20bf3a7594c9e2c6")
                .unwrap();
        let privkey = SecretKey::from_slice(&privkey).unwrap();
        let pubkey = PublicKey::from_secret_key(&secp, &privkey);

        let data = "just test".as_bytes();
        let encrypted_data = encrypt_with_randomness(pubkey, data, &randomness)
            .unwrap()
            .to_hex();
        assert_eq!(
            encrypted_data,
            "6afa9046a9579cad143a384c1b564b9a02ca002079be667ef9dcbb\
	     ac55a06295ce870b07029bfcdb2dce28d959f2815b16f817980020\
	     483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d0\
	     8ffb10d4b8cdf51bd3b9b3aad7f8c5e5b941b18c78105d6445d820\
	     d5c67ece5c010c44f28ca83186e2201bef377b55095e7f2ff483"
        );

        let data = "just test long xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
            .as_bytes();
        let encrypted_data = encrypt_with_randomness(pubkey, data, &randomness)
            .unwrap()
            .to_hex();
        assert_eq!(
            encrypted_data,
            "6afa9046a9579cad143a384c1b564b9a02ca002079be667ef9dcbb\
	     ac55a06295ce870b07029bfcdb2dce28d959f2815b16f817980020\
	     483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d0\
	     8ffb10d4b843077f22260b11a2d82f8dd4dffd6205c86585da0e11\
	     c955a292855195121bf3105d41d63ea884e83b9706872b8ef29101\
	     4e4f5911143430ddaf9d03e29ed3cc64a5328073f8a2a714913b2c\
	     78b113ff990547201ad9c4c50533ef4cdb40e7a61bccdfcbd5c93d\
	     9d76a559fed3e7d017"
        );
    }
}
