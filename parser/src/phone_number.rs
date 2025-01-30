use nom::bytes::complete::take_while;
use nom::character::complete::char as nom_char;
use nom::sequence::preceded;
use nom::IResult;
use nom::Parser;

fn is_digit_or_symbol(c: char) -> bool {
    c.is_ascii_digit() || c.is_whitespace() || ".-/()[]".contains(c)
}

pub(crate) fn phone_number(s: &str) -> IResult<&str, String> {
    let (s, digits_and_symbols) =
        preceded(nom_char('+'), take_while(is_digit_or_symbol)).parse(s)?;
    let digits = digits_and_symbols
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    Ok((s, digits))
}
