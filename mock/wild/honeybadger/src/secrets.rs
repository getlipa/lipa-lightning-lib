pub fn generate_mnemonic() -> Vec<String> {
    vec![
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "abandon".to_string(),
        "cactus".to_string(),
    ]
}

#[derive(Clone)]
pub struct KeyPair {
    pub secret_key: String,
    pub public_key: String,
}

pub struct WalletKeys {
    pub wallet_keypair: KeyPair,
}
pub fn generate_keypair() -> KeyPair {
    KeyPair {
        secret_key: "secret_key_dummy".to_string(),
        public_key: "public_key_dummy".to_string(),
    }
}
