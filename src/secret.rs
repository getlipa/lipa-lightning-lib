/// An object that holds necessary secrets. Should be dealt with carefully and never be logged.
///
/// The consumer of the library *must* persist `mnemonic` and `passphrase`
/// *securely* on the device,
/// The consumer of the library *must* never use or share it except to display it to
/// the end user for backup or for recovering a wallet.
/// The consumer of the library may want to *securely* persist `seed` or derive it
/// every time `seed` is needed, but it will have performance implications.
#[derive(PartialEq, Eq, Debug)]
pub struct Secret {
    /// The 24 words used to derive the node's private key
    pub mnemonic: Vec<String>,
    /// Optional passphrase. If not provided, it is an empty string.
    pub passphrase: String,
    /// The seed derived from the mnemonic and the passphrase
    pub seed: Vec<u8>,
}

impl Secret {
    pub fn get_seed_as_array(&self) -> [u8; 64] {
        let mut seed = [0u8; 64];
        seed.copy_from_slice(&self.seed[..64]);
        seed
    }
}
