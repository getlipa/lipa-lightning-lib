#[derive(PartialEq, Eq, Debug)]
pub struct Secret {
    pub mnemonic: Vec<String>,
    pub passphrase: String,
    pub seed: Vec<u8>,
}

impl Secret {
    pub fn get_seed_as_array(&self) -> [u8; 64] {
        let mut seed = [0u8; 64];
        seed.copy_from_slice(&self.seed[..64]);
        seed
    }
}
