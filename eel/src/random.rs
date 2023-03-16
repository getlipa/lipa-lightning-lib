use crate::errors::Result;

use cipher::generic_array::{ArrayLength, GenericArray};
use perro::MapToError;
use rand::rngs::OsRng;
use rand::RngCore;

pub(crate) fn generate_random_bytes<N: ArrayLength<u8>>() -> Result<GenericArray<u8, N>> {
    let mut bytes = GenericArray::<u8, N>::default();
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_to_permanent_failure("Failed to generate random bytes")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cipher::generic_array::typenum::U32;

    #[test]
    fn test_random_bytes_generation() {
        let bytes_not_filled = [0u8; 32];
        let bytes_non_random = [255u8; 32];

        let bytes = generate_random_bytes::<U32>().unwrap();

        assert_eq!(bytes.len(), 32);
        assert_ne!(bytes.as_ref(), bytes_not_filled);
        assert_ne!(bytes.as_ref(), bytes_non_random);
    }
}
