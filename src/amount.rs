use crate::ExchangeRate;
use std::time::SystemTime;

/// A fiat value accompanied by the exchange rate that was used to get it.
pub struct FiatValue {
    /// Fiat amount denominated in the currencies' minor units. For most fiat currencies, the minor unit is the cent.
    pub minor_units: u64,
    pub currency_code: String,
    /// Sats per major unit
    pub rate: u32,
    pub converted_at: SystemTime,
}

/// A sat amount accompanied by its fiat value in a specific fiat currency
pub struct Amount {
    pub sats: u64,
    pub fiat: Option<FiatValue>,
}

pub(crate) trait ToAmount {
    fn to_amount_up(self, rate: &Option<ExchangeRate>) -> Amount;
    fn to_amount_down(self, rate: &Option<ExchangeRate>) -> Amount;
}

impl ToAmount for u64 {
    fn to_amount_up(self, rate: &Option<ExchangeRate>) -> Amount {
        msats_to_amount(Rounding::Up, self, rate)
    }

    fn to_amount_down(self, rate: &Option<ExchangeRate>) -> Amount {
        msats_to_amount(Rounding::Down, self, rate)
    }
}

#[derive(Copy, Clone)]
enum Rounding {
    Up,
    Down,
}

fn round(msat: u64, rounding: Rounding) -> u64 {
    match rounding {
        Rounding::Up => (msat + 999) / 1_000,
        Rounding::Down => msat / 1_000,
    }
}

fn msats_to_amount(rounding: Rounding, msats: u64, rate: &Option<ExchangeRate>) -> Amount {
    let sats = round(msats, rounding);
    let fiat = rate.as_ref().map(|rate| FiatValue {
        minor_units: round(msats * 100 / rate.rate as u64, rounding),
        currency_code: rate.currency_code.clone(),
        rate: rate.rate,
        converted_at: rate.updated_at,
    });
    Amount { sats, fiat }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn rounding_up() {
        assert_eq!(round(0, Rounding::Up), 0);

        for i in 1..1000 {
            assert_eq!(round(i, Rounding::Up), 1);
        }

        assert_eq!(round(1001, Rounding::Up), 2);
    }

    #[test]
    pub fn rounding_down() {
        for i in 0..1000 {
            assert_eq!(round(i, Rounding::Down), 0);
        }

        assert_eq!(round(1000, Rounding::Down), 1);
    }

    #[test]
    pub fn rounding_to_amount_up() {
        let now = SystemTime::now();
        let amount = 12349123u64.to_amount_up(&None);
        assert_eq!(amount.sats, 12350);
        assert!(amount.fiat.is_none());

        let rate = ExchangeRate {
            currency_code: "EUR".to_string(),
            rate: 4256,
            updated_at: now,
        };
        let amount = 12349123u64.to_amount_up(&Some(rate));
        assert_eq!(amount.sats, 12350);
        assert!(amount.fiat.is_some());
        let fiat = amount.fiat.unwrap();
        assert_eq!(fiat.currency_code, "EUR");
        assert_eq!(fiat.minor_units, 291);
        assert_eq!(fiat.converted_at, now);
    }

    #[test]
    pub fn rounding_to_amount_down() {
        let now = SystemTime::now();
        let amount = 12349123u64.to_amount_down(&None);
        assert_eq!(amount.sats, 12349);
        assert!(amount.fiat.is_none());

        let rate = ExchangeRate {
            currency_code: "EUR".to_string(),
            rate: 4256,
            updated_at: now,
        };
        let amount = 12349123u64.to_amount_down(&Some(rate));
        assert_eq!(amount.sats, 12349);
        assert!(amount.fiat.is_some());
        let fiat = amount.fiat.unwrap();
        assert_eq!(fiat.currency_code, "EUR");
        assert_eq!(fiat.minor_units, 290);
        assert_eq!(fiat.converted_at, now);
    }
}
