use crate::errors::ParsePhoneNumberError;
use perro::ensure;
use phonenumber::country::Id as CountryCode;
use phonenumber::ParseError;

#[derive(PartialEq, Debug)]
pub struct PhoneNumber {
    pub e164: String,
    pub country_code: CountryCode,
}

impl PhoneNumber {
    pub(crate) fn parse(number: &str) -> Result<PhoneNumber, ParsePhoneNumberError> {
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
        Ok(PhoneNumber { e164, country_code })
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

#[cfg(test)]
mod tests {
    use super::*;

    static LIPA_DOMAIN: &str = "@lipa.swiss";

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
