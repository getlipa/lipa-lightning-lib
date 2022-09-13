/// An object that holds necessary secrets.
///
/// # Fields:
///
/// * `mnemonic` - a mnemonic code or mnemonic sentence as described in BIP-39.
/// * `passphrase` - an optional word (a sentence) added to the mnemonic.
/// * `seed` - a seed one way derived from the mnemonic and the passphrase.
///
/// The user of the library *must* persist `mnemonic` and `passphrase`
/// *securely* on the device,
/// The user of the library *must* never use or share it except to display it to
/// the end user for backup or recover a wallet.
/// The user of the library may want to *securely* persist `seed` or derive it
/// every time `seed` is needed, but it will have performance implications.
pub struct Secret {
    pub mnemonic: Vec<String>,
    pub passphrase: String,
    pub seed: Vec<u8>,
}
