pub trait ToSats {
    fn to_sats_up(self) -> Self;
    fn to_sats_down(self) -> Self;
}

impl ToSats for u64 {
    fn to_sats_up(self) -> Self {
        if self % 1_000 == 0 {
            self / 1_000
        } else {
            self / 1_000 + 1
        }
    }

    fn to_sats_down(self) -> Self {
        self / 1_000
    }
}

#[cfg(test)]
mod tests {
    use crate::rounding::ToSats;

    #[test]
    #[rustfmt::skip]
    fn test_rounding_to_sats_down() {
	assert_eq!(     0.to_sats_down(), 0);
	assert_eq!(     1.to_sats_down(), 0);
	assert_eq!(     2.to_sats_down(), 0);
	assert_eq!(   999.to_sats_down(), 0);
	assert_eq!(1_000.to_sats_down(),  1);
	assert_eq!(1_001.to_sats_down(),  1);
	assert_eq!(1_002.to_sats_down(),  1);
	assert_eq!(1_999.to_sats_down(),  1);
	assert_eq!(2_000.to_sats_down(),  2);
	assert_eq!(1_234_567.to_sats_down(), 1_234);
    }

    #[test]
    #[rustfmt::skip]
    fn test_rounding_to_sats_up() {
	assert_eq!(     0.to_sats_up(), 0);
	assert_eq!(     1.to_sats_up(), 1);
	assert_eq!(     2.to_sats_up(), 1);
	assert_eq!(   999.to_sats_up(), 1);
	assert_eq!(1_000.to_sats_up(),  1);
	assert_eq!(1_001.to_sats_up(),  2);
	assert_eq!(1_002.to_sats_up(),  2);
	assert_eq!(1_999.to_sats_up(),  2);
	assert_eq!(2_000.to_sats_up(),  2);
	assert_eq!(1_234_567.to_sats_up(), 1_235);
    }
}
