pub fn round_down_to_sat(amount_msat: u64) -> u64 {
    amount_msat / 1000
}

pub fn round_up_to_sat(amount_msat: u64) -> u64 {
    (amount_msat + 999) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_rounding_msat_down_to_satoshi() {
        for i in 0..1000 {
            assert_eq!(round_down_to_sat(i), 0);
        }

        assert_eq!(round_down_to_sat(1000), 1);
    }

    #[test]
    pub fn test_rounding_msat_up_to_satoshi() {
        assert_eq!(round_up_to_sat(0), 0);

        for i in 1..=1000 {
            assert_eq!(round_up_to_sat(i), 1);
        }
    }
}
