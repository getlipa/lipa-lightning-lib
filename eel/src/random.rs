use crate::errors::Result;

use perro::MapToError;
use rand::rngs::OsRng;
use rand::RngCore;

pub(crate) fn generate_random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_to_permanent_failure("Failed to generate random bytes")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_bytes_generation() {
        let bytes_not_filled = [0u8; 32];
        let bytes_non_random = [255u8; 32];

        let bytes = generate_random_bytes::<32>().unwrap();

        assert_eq!(bytes.len(), 32);
        assert_ne!(bytes, bytes_not_filled);
        assert_ne!(bytes, bytes_non_random);
    }
}
