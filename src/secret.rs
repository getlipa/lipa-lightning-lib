#[derive(PartialEq, Eq, Debug)]
pub struct Secret {
    pub mnemonic: Vec<String>,
    pub passphrase: String,
    pub seed: Vec<u8>,
}
