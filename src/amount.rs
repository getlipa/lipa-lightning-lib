use chrono::{DateTime, Utc};
use eel::interfaces::ExchangeRate;
use std::{fmt, time::SystemTime};

pub struct FiatValue {
    pub minor_units: u64,
    pub currency_code: String,
    pub updated_at: SystemTime,
}

pub struct Amount {
    pub sats: u64,
    pub fiat: Option<FiatValue>,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} sats", self.sats)?;
        if let Some(fiat) = &self.fiat {
            let dt: DateTime<Utc> = fiat.updated_at.into();
            write!(
                f,
                " ({:.2} {} as of {})",
                fiat.minor_units as f64 / 100f64,
                fiat.currency_code,
                dt.format("%d/%m/%Y %T UTC"),
            )?;
        }
        Ok(())
    }
}

pub(crate) trait ToAmount {
    fn to_amount_up(self, rate: Option<ExchangeRate>) -> Amount;
    fn to_amount_down(self, rate: Option<ExchangeRate>) -> Amount;
}

impl ToAmount for u64 {
    fn to_amount_up(self, rate: Option<ExchangeRate>) -> Amount {
        msats_to_amount(Rounding::Up, self, rate)
    }

    fn to_amount_down(self, rate: Option<ExchangeRate>) -> Amount {
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
        Rounding::Up => {
            if msat % 1_000 == 0 {
                msat / 1_000
            } else {
                msat / 1_000 + 1
            }
        }
        Rounding::Down => msat / 1_000,
    }
}

fn msats_to_amount(rounding: Rounding, msats: u64, rate: Option<ExchangeRate>) -> Amount {
    let sats = round(msats, rounding);
    let fiat = rate.map(|rate| FiatValue {
        minor_units: round(msats * 100 / rate.rate as u64, rounding),
        currency_code: rate.currency_code,
        updated_at: rate.updated_at,
    });
    Amount { sats, fiat }
}
