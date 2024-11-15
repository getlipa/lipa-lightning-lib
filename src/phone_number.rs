use crate::errors::{ParsePhoneNumberError, ParsePhoneNumberPrefixError};
use perro::ensure;
use phonenumber::country::Id as CountryCode;
use phonenumber::metadata::DATABASE;
use phonenumber::ParseError;

#[derive(PartialEq, Debug)]
pub struct PhoneNumber {
    pub e164: String,
    pub country_code: CountryCode,
}

impl PhoneNumber {
    pub(crate) fn parse(number: &str) -> Result<Self, ParsePhoneNumberError> {
        let number = match phonenumber::parse(None, number) {
            Ok(number) => number,
            Err(ParseError::InvalidCountryCode) => {
                return Err(ParsePhoneNumberError::MissingCountryCode)
            }
            Err(_) => return Err(ParsePhoneNumberError::ParsingError),
        };
        ensure!(number.is_valid(), ParsePhoneNumberError::InvalidPhoneNumber);

        let e164 = number.format().mode(phonenumber::Mode::E164).to_string();
        let country_code = number
            .country()
            .id()
            .ok_or(ParsePhoneNumberError::InvalidCountryCode)?;
        Ok(Self { e164, country_code })
    }

    pub(crate) fn to_lightning_address(&self, domain: &str) -> String {
        self.e164.replacen('+', "-", 1) + domain
    }
}

pub(crate) fn lightning_address_to_phone_number(address: &str, domain: &str) -> Option<String> {
    let username = address
        .strip_prefix('-')
        .and_then(|s| s.strip_suffix(domain));
    if let Some(username) = username {
        if username.chars().all(|c| char::is_ascii_digit(&c)) {
            return Some(format!("+{username}"));
        }
    }
    None
}

#[derive(Clone)]
pub(crate) struct PhoneNumberPrefixParser {
    allowed_country_codes: Vec<String>,
}

impl PhoneNumberPrefixParser {
    pub fn new(allowed_countries_iso_3166_1_alpha_2: &[String]) -> Self {
        // Stricly speaking *ISO 3166-1 alpha-2* is not the same as *CLDR country IDs*
        // and such conversion is not correct, but for the most contries such codes match.
        let allowed_country_codes = allowed_countries_iso_3166_1_alpha_2
            .iter()
            .flat_map(|id| DATABASE.by_id(id))
            .map(|m| m.country_code().to_string())
            .collect::<Vec<_>>();
        Self {
            allowed_country_codes,
        }
    }

    pub fn parse(&self, prefix: &str) -> Result<(), ParsePhoneNumberPrefixError> {
        match parser::parse_phone_number(prefix) {
            Ok(digits) => {
                if self
                    .allowed_country_codes
                    .iter()
                    .any(|c| digits.starts_with(c))
                {
                    Ok(())
                } else if self
                    .allowed_country_codes
                    .iter()
                    .any(|c| c.starts_with(&digits))
                {
                    Err(ParsePhoneNumberPrefixError::Incomplete)
                } else {
                    Err(ParsePhoneNumberPrefixError::UnsupportedCountry)
                }
            }
            Err(parser::ParseError::Incomplete) => Err(ParsePhoneNumberPrefixError::Incomplete),
            Err(
                parser::ParseError::UnexpectedCharacter(index)
                | parser::ParseError::ExcessSuffix(index),
            ) => Err(ParsePhoneNumberPrefixError::InvalidCharacter { at: index as u32 }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static LIPA_DOMAIN: &str = "@lipa.swiss";

    #[test]
    fn test_parse_phone_number_prefix() {
        let ch = PhoneNumberPrefixParser::new(&["CH".to_string()]);
        assert_eq!(ch.parse(""), Err(ParsePhoneNumberPrefixError::Incomplete));
        assert_eq!(ch.parse("+"), Err(ParsePhoneNumberPrefixError::Incomplete));
        assert_eq!(
            ch.parse("+3"),
            Err(ParsePhoneNumberPrefixError::UnsupportedCountry)
        );
        assert_eq!(ch.parse("+4"), Err(ParsePhoneNumberPrefixError::Incomplete));
        assert_eq!(
            ch.parse("+4 "),
            Err(ParsePhoneNumberPrefixError::Incomplete)
        );
        assert_eq!(
            ch.parse("+44"),
            Err(ParsePhoneNumberPrefixError::UnsupportedCountry)
        );
        assert_eq!(ch.parse("+41"), Ok(()));
        assert_eq!(ch.parse("+41 ("), Ok(()));
        assert_eq!(ch.parse("+41 (935"), Ok(()));
        assert_eq!(
            ch.parse("+41a"),
            Err(ParsePhoneNumberPrefixError::InvalidCharacter { at: 3 })
        );

        let us = PhoneNumberPrefixParser::new(&["US".to_string()]);
        assert_eq!(us.parse("+"), Err(ParsePhoneNumberPrefixError::Incomplete));
        assert_eq!(us.parse("+1"), Ok(()));
        assert_eq!(us.parse("+12"), Ok(()));

        let us_and_ch = PhoneNumberPrefixParser::new(&["US".to_string(), "CH".to_string()]);
        assert_eq!(
            us_and_ch.parse("+"),
            Err(ParsePhoneNumberPrefixError::Incomplete)
        );
        assert_eq!(us_and_ch.parse("+1"), Ok(()));
        assert_eq!(us_and_ch.parse("+12"), Ok(()));
        assert_eq!(
            us_and_ch.parse("+3"),
            Err(ParsePhoneNumberPrefixError::UnsupportedCountry)
        );
        assert_eq!(
            us_and_ch.parse("+4"),
            Err(ParsePhoneNumberPrefixError::Incomplete)
        );
        assert_eq!(
            us_and_ch.parse("+44"),
            Err(ParsePhoneNumberPrefixError::UnsupportedCountry)
        );
        assert_eq!(us_and_ch.parse("+41"), Ok(()));
    }

    #[test]
    fn test_parse_phone_number() {
        let expected = PhoneNumber {
            e164: "+41446681800".to_string(),
            country_code: CountryCode::CH,
        };
        assert_eq!(PhoneNumber::parse("+41 44 668 18 00").unwrap(), expected);
        assert_eq!(
            PhoneNumber::parse("tel:+41-44-668-18-00").unwrap(),
            expected,
        );
        assert_eq!(PhoneNumber::parse("+41446681800").unwrap(), expected);

        assert_eq!(
            PhoneNumber::parse("044 668 18 00").unwrap_err(),
            ParsePhoneNumberError::MissingCountryCode
        );
        assert_eq!(
            PhoneNumber::parse("446681800").unwrap_err(),
            ParsePhoneNumberError::MissingCountryCode
        );
        // Missing the last digit.
        assert_eq!(
            PhoneNumber::parse("+41 44 668 18 0").unwrap_err(),
            ParsePhoneNumberError::InvalidPhoneNumber
        );
    }

    #[test]
    fn test_to_from_lightning_address_e2e() {
        let original = PhoneNumber::parse("+41 44 668 18 00").unwrap();
        let address = original.to_lightning_address(LIPA_DOMAIN);
        let e164 = lightning_address_to_phone_number(&address, LIPA_DOMAIN).unwrap();
        let result = PhoneNumber::parse(&e164).unwrap();
        assert_eq!(original.e164, result.e164);
    }

    #[test]
    fn test_to_from_lightning_address() {
        assert_eq!(
            PhoneNumber::parse("+41 44 668 18 00")
                .unwrap()
                .to_lightning_address(LIPA_DOMAIN),
            "-41446681800@lipa.swiss",
        );

        assert_eq!(
            lightning_address_to_phone_number("-41446681800@lipa.swiss", LIPA_DOMAIN).unwrap(),
            "+41446681800"
        );
        assert_eq!(
            lightning_address_to_phone_number("41446681800@lipa.swiss", LIPA_DOMAIN),
            None
        );
        assert_eq!(
            lightning_address_to_phone_number("-41446681800@other.domain", LIPA_DOMAIN),
            None
        );
        assert_eq!(
            lightning_address_to_phone_number("-4144668aa1800@lipa.swiss", LIPA_DOMAIN),
            None
        );
    }
}
