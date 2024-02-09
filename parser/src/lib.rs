mod domain;
mod lightning_address;

use lightning_address::lightning_address;
use nom::character::complete::space0;
use nom::error::Error;
use nom::sequence::delimited;
use nom::Finish;

/// Enum representing possible errors why parsing could fail.
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// Parsing failed because parsed string was not complete.
    /// Additional characters are needed to make the string valid.
    /// It makes parsed string a valid prefix of a valid string.
    Incomplete,

    /// Parsing failed because an unexpected character at the position was met.
    /// The character *has to be removed*.
    UnexpectedCharacter(usize),

    /// Parsing failed because an excess suffix at the position was met.
    /// The suffix *has to be removed*.
    ExcessSuffix(usize),
}

pub fn parse_lightning_address(address: &str) -> Result<(), ParseError> {
    let r = delimited(space0, lightning_address, space0)(address).finish();
    match r {
        Ok(("", ())) => Ok(()),
        Ok((rem, ())) => Err(ParseError::ExcessSuffix(address.len() - rem.len())),
        Err(Error { input: "", .. }) => Err(ParseError::Incomplete),
        Err(Error { input, .. }) => {
            Err(ParseError::UnexpectedCharacter(address.len() - input.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_lightning_address as p;
    use super::*;

    #[test]
    fn test_parse_lightning_address() {
        assert_eq!(p(""), Err(ParseError::Incomplete));
        assert_eq!(p("  "), Err(ParseError::Incomplete));
        assert_eq!(p("  a@l"), Err(ParseError::Incomplete));

        assert_eq!(p("a"), Err(ParseError::Incomplete));
        assert_eq!(p("ab"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a."), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.u"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.uk"), Ok(()));
        assert_eq!(p("ab@a.uk."), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.uk.c"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.uk.co"), Ok(()));
        assert_eq!(p("ab@a.uk.com"), Ok(()));
        assert_eq!(p("a_b-@1m-l.com"), Ok(()));
        assert_eq!(p(".@1.ch"), Ok(()));
    }

    #[test]
    fn test_top_level_domains_are_alphabetic_only() {
        assert_eq!(p("ab@a1."), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.2"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.2s"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.2sf."), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.2sf.f"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.2sfds"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.u"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a1.u2"), Err(ParseError::Incomplete));
    }

    #[test]
    fn test_surrounding_spaces_are_tolerated_for_complete_addresses() {
        assert_eq!(p(" ab@a.uk      "), Ok(()));
        assert_eq!(p("   ab@a.uk.com"), Ok(()));
        assert_eq!(p("ab@a.uk.com   "), Ok(()));
    }

    #[test]
    fn test_errors() {
        assert_eq!(p("ü"), Err(ParseError::UnexpectedCharacter(0)));
        assert_eq!(p("ы"), Err(ParseError::UnexpectedCharacter(0)));
        assert_eq!(p("@"), Err(ParseError::UnexpectedCharacter(0)));
        assert_eq!(p("a:"), Err(ParseError::UnexpectedCharacter(1)));
        assert_eq!(p("a@ a"), Err(ParseError::UnexpectedCharacter(2)));
        assert_eq!(p("a@."), Err(ParseError::UnexpectedCharacter(2)));
        assert_eq!(p("a@@"), Err(ParseError::UnexpectedCharacter(2)));
        assert_eq!(p("a@a.."), Err(ParseError::UnexpectedCharacter(4)));
        assert_eq!(p("a@a_"), Err(ParseError::UnexpectedCharacter(3)));
        assert_eq!(p("ab@a.!"), Err(ParseError::UnexpectedCharacter(5)));
        assert_eq!(p("ab@lipa.swiss!"), Err(ParseError::ExcessSuffix(13)));
        assert_eq!(p("  ab@a.uk.com  c"), Err(ParseError::ExcessSuffix(15)));
    }

    #[test]
    fn test_internationalized_domain_names() {
        assert_eq!(p("a@⚡"), Err(ParseError::Incomplete));
        assert_eq!(p("a@⚡."), Err(ParseError::Incomplete));
        assert_eq!(p("a@⚡.ф"), Err(ParseError::Incomplete));
        assert_eq!(p("a@⚡.фы"), Ok(()));
        assert_eq!(p("a@⚡.Ё"), Err(ParseError::Incomplete));
        // Top-level domain check considers graphemes, not unicode characters.
        assert_eq!(p("a@⚡.y̆"), Err(ParseError::Incomplete));
    }

    #[test]
    fn test_internationalized_domain_names_in_punycode() {
        assert_eq!(p("ab@xn-"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.xn-"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.xn--"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.xn--9"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.xn--90"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.xn--90a"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@a.xn--90aü"), Err(ParseError::UnexpectedCharacter(12)));
        assert_eq!(p("ab@a.xn--90aы"), Err(ParseError::UnexpectedCharacter(12)));
        assert_eq!(p("ab@a.xn--90ae"), Ok(()));
        // `Bücher.example` as `xn--bcher-kva.example`.
        assert_eq!(p("ab@xn--"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--b"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bc"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bch"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bche"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bcher"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bcher-kva"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bcher-kva."), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bcher-kva.e"), Err(ParseError::Incomplete));
        assert_eq!(p("ab@xn--bcher-kva.ex"), Ok(()));
        assert_eq!(p("ab@xn--bcher-kva.example"), Ok(()));
    }

    #[test]
    fn test_hyphens_in_domains() {
        // No double hyphens are allowed.
        assert_eq!(p("ab@1--"), Err(ParseError::UnexpectedCharacter(5)));
        // No leadin/trailing hyphens are allowed in labels.
        assert_eq!(p("ab@-"), Err(ParseError::UnexpectedCharacter(3)));
        assert_eq!(p("ab@a-."), Err(ParseError::UnexpectedCharacter(5)));
    }
}
