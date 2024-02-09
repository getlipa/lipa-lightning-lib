use crate::{ensure, Result};

use perro::invalid_input;

pub(crate) fn strong_type_seed(seed: &[u8]) -> Result<[u8; 64]> {
    ensure!(
        seed.len() == 64,
        invalid_input("Seed must be 64 bytes long")
    );

    let mut seed_array = [0u8; 64];
    seed_array.copy_from_slice(&seed[0..64]);

    Ok(seed_array)
}
