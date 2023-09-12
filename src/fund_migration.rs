use crate::errors::Result;

use bitcoin::hashes::hex::ToHex;
use bitcoin::hashes::sha256;
use bitcoin::secp256k1::{Message, PublicKey, SECP256K1};
use bitcoin::util::bip32::{ChildNumber, ExtendedPrivKey};
use bitcoin::Network;
use perro::MapToError;

#[allow(dead_code)]
fn sign_with_ldk_key(seed: [u8; 64], message: &str) -> Result<(String, String)> {
    let mut first_half = [0u8; 32];
    first_half.copy_from_slice(&seed[..32]);
    // Note that when we aren't serializing the key, network doesn't matter
    let master_key = ExtendedPrivKey::new_master(Network::Testnet, &first_half)
        .map_to_permanent_failure("Failed to build master key")?;
    let node_secret = master_key
        .ckd_priv(SECP256K1, ChildNumber::from_hardened_idx(0).unwrap())
        .map_to_permanent_failure("Failed to build node secret")?
        .private_key;

    let message = format!("I want to payout my funds to {message}");
    let message = Message::from_hashed_data::<sha256::Hash>(message.as_bytes());
    let signature = node_secret.sign_ecdsa(message).serialize_der().to_string();

    let node_id = PublicKey::from_secret_key(SECP256K1, &node_secret)
        .serialize()
        .to_hex();
    Ok((node_id, signature))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::ecdsa::Signature;
    use std::str::FromStr;

    #[test]
    fn test_signing() {
        let seed = [0u8; 64];
        let (node_id, signature) = sign_with_ldk_key(seed, "invoice").unwrap();
        let pub_key = PublicKey::from_str(&node_id).unwrap();
        let signature = Signature::from_str(&signature).unwrap();
        let message = "I want to payout my funds to invoice".as_bytes();
        let message = Message::from_hashed_data::<sha256::Hash>(message);
        signature.verify(&message, &pub_key).unwrap();
    }
}
