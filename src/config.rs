use bitcoin::Network;

/// An object that holds all configuration needed to start a LightningNode instance.
///
/// # Fields:
///
/// * `seed` - the seed derived from the mnemonic and optional pass phrase.
pub struct Config {
    pub network: Network,
    pub seed: Vec<u8>,
}
