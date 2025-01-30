use nom::bytes::complete::{tag_no_case, take_while1};
use nom::character::complete::char as nom_char;
use nom::error::Error;
use nom::multi::separated_list1;
use nom::sequence::preceded;
use nom::{IResult, Parser};
use unicode_segmentation::UnicodeSegmentation;

fn is_label_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || !c.is_ascii()
}

fn is_punycode_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

enum Label<'a> {
    Unicode(&'a str),
    UnicodeWithHyphens,
    Punycode(&'a str),
}

fn punycode_label(s: &str) -> IResult<&str, Label> {
    take_while1(is_punycode_char).map(Label::Punycode).parse(s)
}

fn unicode_label(s: &str) -> IResult<&str, Label> {
    // Leading, trailing, or doubled hyphens are not allowed.
    let (s, parts) = separated_list1(nom_char('-'), take_while1(is_label_char)).parse(s)?;
    if s.starts_with('-') {
        return preceded(
            nom_char('-'),
            take_while1(is_label_char).map(|_| Label::UnicodeWithHyphens),
        )
        .parse(s);
    }
    match parts.as_slice() {
        [] => Err(nom::Err::Failure(Error {
            input: s,
            code: nom::error::ErrorKind::Fail,
        })),
        [part] => Ok((s, Label::Unicode(part))),
        _ => Ok((s, Label::UnicodeWithHyphens)),
    }
}

fn label(s: &str) -> IResult<&str, Label> {
    let r: IResult<_, _> = tag_no_case("xn--")(s);
    match r {
        Ok((s, _tag)) => punycode_label(s),
        Err(_) => unicode_label(s),
    }
}

fn is_valid_top_level_domain(label: &Label) -> bool {
    match label {
        Label::Unicode(label) => {
            label.graphemes(true).count() >= 2
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || !c.is_ascii())
        }
        Label::UnicodeWithHyphens => false,
        Label::Punycode(label) => label.chars().count() >= 4 && label.chars().all(is_punycode_char),
    }
}

pub(crate) fn domain(s: &str) -> IResult<&str, ()> {
    let (s, labels) = separated_list1(nom_char('.'), label).parse(s)?;
    if !s.starts_with('.') && labels.len() > 1 {
        if let Some(label) = labels.last() {
            if is_valid_top_level_domain(label) {
                return Ok((s, ()));
            }
        }
    }
    preceded(nom_char('.'), label.map(std::mem::drop)).parse(s)
}
