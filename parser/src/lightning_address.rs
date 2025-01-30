use super::domain::domain;

use nom::bytes::complete::take_while1;
use nom::character::complete::char as nom_char;
use nom::sequence::separated_pair;
use nom::IResult;
use nom::Parser;

fn is_username_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

pub(crate) fn lightning_address(s: &str) -> IResult<&str, ()> {
    let username = take_while1(is_username_char);
    let (s, (_username, _domain)) = separated_pair(username, nom_char('@'), domain).parse(s)?;
    Ok((s, ()))
}
