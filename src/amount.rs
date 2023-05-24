use eel::interfaces::ExchangeRate;
use eel::payment::FiatValues;
use std::fmt;

#[derive(Debug)]
pub struct Amount {
    pub sats: u64,
    pub fiat: Option<FiatValues>,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} sats", self.sats)?;
        if let Some(fiat) = &self.fiat {
            write!(
                f,
                " ({:.2} {}, {:.2} USD)",
                fiat.amount as f64 / 1000f64,
                fiat.fiat,
                fiat.amount_usd as f64 / 1000f64,
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
    let fiat = rate.map(|rate| FiatValues::from_amount_msat(msats, &rate, &rate));
    Amount { sats, fiat }
}
