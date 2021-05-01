use nom::{
    bytes::complete::{is_not, tag, take, take_until, take_while},
    combinator::opt,
    sequence::{preceded, terminated},
    Err, IResult, Parser,
};

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum ParseError<'a> {
    #[error("Parser failed at {0}")]
    NomError(&'a str, nom::error::ErrorKind),
    #[error("Parser failed at {0} due to {1}")]
    ErrorKind(&'a str, ErrorKind<'a>),
}

#[derive(Error, Debug, Clone)]
pub enum ErrorKind<'a> {
    #[error("{0}")]
    ConstError(&'static str),
    #[error("undefined parameter {0}")]
    UndefinedParameterError(&'a str),
}

impl<'a> ParseError<'a> {
    pub fn const_error(input: &'a str, reason: &'static str) -> ParseError<'a> {
        ParseError::ErrorKind(input, ErrorKind::ConstError(reason))
    }
    pub fn error_kind(input: &'a str, kind: ErrorKind<'a>) -> ParseError<'a> {
        ParseError::ErrorKind(input, kind)
    }
}

impl<'a> nom::error::ParseError<&'a str> for ParseError<'a> {
    fn from_error_kind(input: &'a str, kind: nom::error::ErrorKind) -> Self {
        ParseError::NomError(input, kind)
    }

    fn append(_input: &'a str, _kind: nom::error::ErrorKind, other: Self) -> Self {
        other
    }
}

pub type PResult<'a, O> = IResult<&'a str, O, ParseError<'a>>;

pub fn space(input: &str) -> PResult<&str> {
    opt(take_while(|chr: char| chr.is_whitespace()))(input)
        .map(|(input, val)| (input, val.unwrap_or("")))
}

pub fn dash_comment(input: &str) -> PResult<&str> {
    preceded(tag("--"), opt(is_not("\n")))
        .map(|val| val.unwrap_or(""))
        .parse(input)
}

pub fn slash_comment(input: &str) -> PResult<Vec<&str>> {
    let (input, _): (&str, _) = tag("/*")(input)?;
    let mut end_location = input
        .find("*/")
        .ok_or_else(|| Err::Error(ParseError::const_error(input, "comment is unterminated")))?;

    match input.find('\n') {
        Some(next_line) if next_line < end_location => {
            // multi line comment
            let (mut input, first_line) = is_not("\n")(input)?;
            let mut res = vec![first_line];
            end_location = input.find("*/").unwrap();
            input = &input[1..];
            // while we we're not in the line with the "*/"

            while input.find('\n').map_or(false, |val| val < end_location) {
                // skip until * parse until */
                let (og_input, val) = preceded(preceded(space, tag("*")), is_not("\n"))(input)?;
                res.push(val);
                input = &og_input[1..]; // skip the '\n'
                end_location = input.find("*/").unwrap();
            }

            let (input, final_line_opt) = terminated(
                preceded(space.and(tag("*")), take_until("*/"))
                    .map(Some)
                    .or(take_until("*/").map(|_| None)),
                take("*/".len()),
            )
            .parse(input)?;

            if let Some(final_line) = final_line_opt {
                res.push(final_line)
            }

            Ok((input, res))
        }
        _ => {
            // single line comment
            let (comment_string, rest) = input.split_at(end_location);
            Ok((&rest[2..], vec![comment_string]))
        }
    }
}

// TODO add argtypes and validation
#[allow(dead_code)]
enum ArgType {
    Int,
    Float,
    String,
    Null,
    Union(Vec<ArgType>),
}

pub fn is_alpha_or_underscore(chr: char) -> bool {
    chr.is_alphanumeric() || chr == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_comment_test() {
        let test_str = r#"/* testing
                           * testing
                           * testing
                           * testing 
                           * */ testing"#;

        assert_eq!(slash_comment(test_str).unwrap().0, " testing",);

        let test_str = r#"/* testing
                           * testing
                           * testing
                           * testing */"#;

        assert_eq!(
            slash_comment(test_str)
                .unwrap()
                .1
                .iter()
                .map(|v| v.trim())
                .collect::<Vec<_>>(),
            vec!["testing"; 4]
        );

        let test_str = r#"/* testing */ "#;

        assert_eq!(
            slash_comment(test_str)
                .unwrap()
                .1
                .iter()
                .map(|v| v.trim())
                .collect::<Vec<_>>(),
            vec!["testing"; 1]
        );
    }

    #[test]
    fn dash_comment_test() {
        let test_str = r#"-- testing "#;
        assert_eq!(dash_comment(test_str).unwrap().1, " testing ");
    }
}
