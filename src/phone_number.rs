use crate::errors::Result;
use crate::errors::{ParsePhoneNumberError, ParsePhoneNumberPrefixError};
use crate::locker::Locker;
use crate::support::Support;
use crate::symmetric_encryption::encrypt;
use crate::{with_status, EnableStatus, RuntimeErrorCode};
use perro::{ensure, MapToError};
use phonenumber::country::Id as CountryCode;
use phonenumber::metadata::DATABASE;
use phonenumber::ParseError;
use std::sync::Arc;

pub struct PhoneNumber {
    support: Arc<Support>,
}

impl PhoneNumber {
    pub(crate) fn new(support: Arc<Support>) -> Self {
        Self { support }
    }

    /// Query for a previously verified phone number.
    ///
    /// Requires network: **no**
    pub fn get(&self) -> Result<Option<String>> {
        Ok(self
            .support
            .data_store
            .lock_unwrap()
            .retrieve_lightning_addresses()?
            .into_iter()
            .filter_map(with_status(EnableStatus::Enabled))
            .find(|a| a.starts_with('-'))
            .and_then(|a| {
                lightning_address_to_phone_number(
                    &a,
                    &self
                        .support
                        .node_config
                        .remote_services_config
                        .lipa_lightning_domain,
                )
            }))
    }

    /// Start the verification process for a new phone number. This will trigger an SMS containing
    /// an OTP to be sent to the provided `phone_number`. To conclude the verification process,
    /// the method [`PhoneNumber::verify`] should be called next.
    ///
    /// Parameters:
    /// * `phone_number` - the phone number to be registered. Needs to be checked for validity using
    ///   [PhoneNumber::parse_to_lightning_address].
    ///
    /// Requires network: **yes**
    pub fn register(&self, phone_number: String) -> Result<()> {
        let phone_number = self
            .parse_phone_number(phone_number)
            .map_to_invalid_input("Invalid phone number")?;

        let encrypted_number = encrypt(
            phone_number.e164.as_bytes(),
            &self.support.persistence_encryption_key,
        )?;
        let encrypted_number = hex::encode(encrypted_number);

        self.support
            .rt
            .handle()
            .block_on(pigeon::request_phone_number_verification(
                &self.support.node_config.remote_services_config.backend_url,
                &self.support.async_auth,
                phone_number.e164,
                encrypted_number,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to register phone number",
            )
    }

    /// Finish the verification process for a new phone number.
    ///
    /// Parameters:
    /// * `phone_number` - the phone number to be verified.
    /// * `otp` - the OTP code sent as an SMS to the phone number.
    ///
    /// Requires network: **yes**
    pub fn verify(&self, phone_number: String, otp: String) -> Result<()> {
        let phone_number = self
            .parse_phone_number(phone_number)
            .map_to_invalid_input("Invalid phone number")?;

        self.support
            .rt
            .handle()
            .block_on(pigeon::verify_phone_number(
                &self.support.node_config.remote_services_config.backend_url,
                &self.support.async_auth,
                phone_number.e164.clone(),
                otp,
            ))
            .map_to_runtime_error(
                RuntimeErrorCode::AuthServiceUnavailable,
                "Failed to submit phone number registration otp",
            )?;
        let address = phone_number.to_lightning_address(
            &self
                .support
                .node_config
                .remote_services_config
                .lipa_lightning_domain,
        );
        self.support
            .data_store
            .lock_unwrap()
            .store_lightning_address(&address)
    }

    /// Parse a phone number prefix, check against the list of allowed countries
    /// (set in [`LightningNodeConfig::phone_number_allowed_countries_iso_3166_1_alpha_2`](crate::LightningNodeConfig::phone_number_allowed_countries_iso_3166_1_alpha_2)).
    /// The parser is not strict, it parses some invalid prefixes as valid.
    ///
    /// Requires network: **no**
    pub fn parse_prefix(
        &self,
        phone_number_prefix: String,
    ) -> std::result::Result<(), ParsePhoneNumberPrefixError> {
        self.support
            .phone_number_prefix_parser
            .parse(&phone_number_prefix)
    }

    /// Parse a phone number, check against the list of allowed countries
    /// (set in [`LightningNodeConfig::phone_number_allowed_countries_iso_3166_1_alpha_2`](crate::LightningNodeConfig::phone_number_allowed_countries_iso_3166_1_alpha_2)).
    ///
    /// Returns a possible lightning address, which can be checked for existence
    /// with [`Util::decode_data`](crate::Util::decode_data).
    ///
    /// Requires network: **no**
    pub fn parse_to_lightning_address(
        &self,
        phone_number: String,
    ) -> std::result::Result<String, ParsePhoneNumberError> {
        let phone_number_recipient = self.parse_phone_number(phone_number)?;
        Ok(phone_number_recipient.to_lightning_address(
            &self
                .support
                .node_config
                .remote_services_config
                .lipa_lightning_domain,
        ))
    }

    fn parse_phone_number(
        &self,
        phone_number: String,
    ) -> std::result::Result<PhoneNumberRecipient, ParsePhoneNumberError> {
        let phone_number_recipient = PhoneNumberRecipient::parse(&phone_number)?;
        ensure!(
            self.support
                .allowed_countries_country_iso_3166_1_alpha_2
                .contains(&phone_number_recipient.country_code.as_ref().to_string()),
            ParsePhoneNumberError::UnsupportedCountry
        );
        Ok(phone_number_recipient)
    }
}

#[derive(PartialEq, Debug)]
pub struct PhoneNumberRecipient {
    pub e164: String,
    pub country_code: CountryCode,
}

impl PhoneNumberRecipient {
    pub(crate) fn parse(number: &str) -> std::result::Result<Self, ParsePhoneNumberError> {
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

    pub fn parse(&self, prefix: &str) -> std::result::Result<(), ParsePhoneNumberPrefixError> {
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
        let expected = PhoneNumberRecipient {
            e164: "+41446681800".to_string(),
            country_code: CountryCode::CH,
        };
        assert_eq!(
            PhoneNumberRecipient::parse("+41 44 668 18 00").unwrap(),
            expected
        );
        assert_eq!(
            PhoneNumberRecipient::parse("tel:+41-44-668-18-00").unwrap(),
            expected,
        );
        assert_eq!(
            PhoneNumberRecipient::parse("+41446681800").unwrap(),
            expected
        );

        assert_eq!(
            PhoneNumberRecipient::parse("044 668 18 00").unwrap_err(),
            ParsePhoneNumberError::MissingCountryCode
        );
        assert_eq!(
            PhoneNumberRecipient::parse("446681800").unwrap_err(),
            ParsePhoneNumberError::MissingCountryCode
        );
        // Missing the last digit.
        assert_eq!(
            PhoneNumberRecipient::parse("+41 44 668 18 0").unwrap_err(),
            ParsePhoneNumberError::InvalidPhoneNumber
        );
    }

    #[test]
    fn test_to_from_lightning_address_e2e() {
        let original = PhoneNumberRecipient::parse("+41 44 668 18 00").unwrap();
        let address = original.to_lightning_address(LIPA_DOMAIN);
        let e164 = lightning_address_to_phone_number(&address, LIPA_DOMAIN).unwrap();
        let result = PhoneNumberRecipient::parse(&e164).unwrap();
        assert_eq!(original.e164, result.e164);
    }

    #[test]
    fn test_to_from_lightning_address() {
        assert_eq!(
            PhoneNumberRecipient::parse("+41 44 668 18 00")
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
