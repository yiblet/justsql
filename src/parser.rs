use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take, take_until, take_while},
    combinator::opt,
    multi::fold_many0,
    sequence::{delimited, preceded, terminated},
    Err, IResult, Parser,
};
use std::collections::BTreeMap;
use std::fmt::Write;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum ParseError<'a> {
    #[error("Parser failed at {0} due to {1:?}")]
    ParseError(&'a str, nom::error::ErrorKind),
    #[error("Parser failed at {0} due to {1}")]
    ConstError(&'a str, &'static str),
}

pub fn const_error<'a>(input: &'a str, reason: &'static str) -> ParseError<'a> {
    ParseError::ConstError(input, reason)
}

impl<'a> nom::error::ParseError<&'a str> for ParseError<'a> {
    fn from_error_kind(input: &'a str, kind: nom::error::ErrorKind) -> Self {
        ParseError::ParseError(input, kind)
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
    preceded(tag("--"), is_not("\n"))(input)
}

pub fn slash_comment(input: &str) -> PResult<Vec<&str>> {
    let (input, _): (&str, _) = tag("/*")(input)?;
    let mut end_location = input
        .find("*/")
        .ok_or_else(|| Err::Error(const_error(input, "comment is unterminated")))?;

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

fn string_literal<'a>(input: &'a str) -> PResult<&'a str> {
    let double_quote_literal = delimited(
        tag("\""),
        fold_many0(
            (tag("\\").and(take(1usize)).map(|_| ())).or(is_not("\\\"").map(|_| ())),
            (),
            |_, _| (),
        ),
        tag("\""),
    );
    let single_quote_literal = delimited(
        tag("'"),
        fold_many0(
            (tag("\\").and(take(1usize)).map(|_| ())).or(is_not("\\'").map(|_| ())),
            (),
            |_, _| (),
        ),
        tag("'"),
    );
    let (output, _) = alt((single_quote_literal, double_quote_literal))(input)?;
    Ok((output, &input[..input.len() - output.len()]))
}

pub fn normalize_sql<'a>(mut input: &'a str) -> PResult<(String, BTreeMap<&'a str, usize>)> {
    let mut res = String::with_capacity(input.len());
    let mut map: BTreeMap<&'a str, usize> = BTreeMap::new();

    while input != "" && &input[0..1] != ";" {
        let literal = alt((
            string_literal,
            take_while(|chr: char| !chr.is_whitespace() && chr != ';'),
        ))
        .map(|res| (None, res));
        let replace = preceded(tag("@"), take_while(|chr: char| chr.is_alphanumeric()))
            .map(|res: &str| (Some(res), res));

        let (output, step) = space(input)?;
        res.push_str(step);

        let (output, (arg_opt, step)) = alt((replace, literal))(output)?;
        match arg_opt {
            Some(key) => {
                let arg_number = match map.get(key) {
                    Some(value) => *value,
                    None => {
                        let cur_arg_number = map.len() + 1;
                        map.insert(key, cur_arg_number);
                        cur_arg_number
                    }
                };
                write!(&mut res, "${}", arg_number)
                    .map_err(|_| Err::Error(const_error(output, "failed to insert into string")))?;
            }
            None => {
                res.push_str(step);
            }
        }

        let (output, step) = space(output)?;
        res.push_str(step);

        if input.len() == output.len() {
            panic!("infinite loop on {}", input);
        }
        input = output;
    }

    let (input, _) = delimited(space, opt(tag(";")), space)(input)?;

    res.shrink_to_fit();
    Ok((input, (res, map)))
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

    #[test]
    fn string_literal_test() {
        let test_str = r#""test" "#;
        assert_eq!(string_literal(test_str).unwrap(), (" ", r#""test""#));
    }

    #[test]
    fn normalize_sql_test() {
        let test_str =
            r#"select * from users where id = @id and @email = 'testing 123 @haha' OR 0 = @id"#;
        let (_, (normalized_sql, _)) = normalize_sql(test_str).unwrap();
        assert_eq!(
            normalized_sql,
            "select * from users where id = $0 and $1 = \'testing 123 @haha\' OR 0 = $0",
        );
    }
}
