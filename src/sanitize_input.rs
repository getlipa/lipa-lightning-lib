use crate::Result;
use perro::Error;

pub(crate) fn strong_type_seed(seed: &Vec<u8>) -> Result<[u8; 64]> {
    if seed.len() != 64 {
        return Err(Error::InvalidInput {
            msg: "Seed must be 64 bytes long".to_string(),
        });
    }
    let mut seed_array = [0u8; 64];
    seed_array.copy_from_slice(&seed[0..64]);

    Ok(seed_array)
}
