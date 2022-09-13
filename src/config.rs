/// An object that holds all configuration needed to start a LightningNode instance.
///
/// # Fields:
///
/// * `secret_seed` - the secret seed as a vector of length 32
pub struct Config {
    pub secret_seed: Vec<u8>,
}
