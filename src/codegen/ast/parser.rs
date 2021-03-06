use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take, take_till, take_while},
    character::complete::satisfy,
    combinator::{cut, eof, opt, peek},
    multi::{fold_many0, separated_list0},
    sequence::delimited,
    Err, Parser,
};

use crate::codegen::result::{PResult, ParseError};

// all space character except for new lines
pub fn line_space0(input: &str) -> PResult<&str> {
    take_while(|c: char| c.is_whitespace() && c != '\n').parse(input)
}

// all space character except for new lines
pub fn line_space1(input: &str) -> PResult<&str> {
    take_while::<_, _, ParseError>(|c: char| c.is_whitespace() && c != '\n')
        .parse(input)
        .map_err(|_| {
            Err::Error(ParseError::const_error(
                input,
                "must have at least one space",
            ))
        })
}

pub fn space(input: &str) -> PResult<&str> {
    take_while(|chr: char| chr.is_whitespace())(input)
}

pub fn string_literal<'a>(input: &'a str) -> PResult<&'a str> {
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

///  parses decorator inside single line comment
///  examples:
///     -- <parser>
///     // <parser>
pub fn with_single_line_comment<'a, P, O>(
    mut parser: P,
) -> impl FnMut(&'a str) -> PResult<Option<O>>
where
    P: Parser<&'a str, O, ParseError<'a>>,
{
    move |input: &'a str| {
        let (input, _) = tag("--").or(tag("//")).and(line_space0).parse(input)?;
        let (input, output) = (|i| parser.parse(i))
            .map(Some)
            .or(take_till(|c| c == '\n').map(|_| None))
            .parse(input)?;
        let (input, _) = cut(line_space0.and(
            nom::character::complete::char('\n')
                .map(|_| ())
                .or(eof.map(|_| ())),
        ))
        .parse(input)?;
        Ok((input, output))
    }
}

/// parsers decorator inside multi-line comment
/// tests for:
///     /* <parser> */
///     /*
///      * <parser> */,
///
///     /* <parser>
///      * <parser>
///      */
///
///     /* <parser>
///        <parser>
///      */

pub fn with_multi_line_comment<'a, P, O>(mut parser: P) -> impl FnMut(&'a str) -> PResult<Vec<O>>
where
    P: Parser<&'a str, O, ParseError<'a>>,
{
    fn start(input: &str) -> PResult<()> {
        tag("/*").and(line_space0).map(|_| ()).parse(input)
    }
    fn line_end(input: &str) -> PResult<()> {
        line_space0
            .and(nom::character::complete::char('\n'))
            .map(|_| ())
            .parse(input)
    }
    fn sep(input: &str) -> PResult<()> {
        tag("*")
            .and(peek(satisfy(|c: char| c != '/')))
            .map(|_| ())
            .parse(input)
    }

    fn inactive_comment(input: &str) -> PResult<()> {
        let mut prev = None;
        let mut pos = None;
        for (idx, chr) in input.char_indices() {
            match (prev, chr) {
                (_, '\n') => {
                    pos = Some(idx);
                    break;
                }
                (Some((idx_prev, '*')), '/') => {
                    pos = Some(idx_prev);
                    break;
                }
                _ => prev = Some((idx, chr)),
            }
        }

        let (_, rest) = input.split_at(pos.ok_or_else(|| {
            Err::Error(ParseError::const_error(
                input,
                "couldn't find end of comment line",
            ))
        })?);
        Ok((rest, ()))
    }

    let mut delimiter = line_end
        .and(line_space0.and(opt(sep)).and(line_space0))
        .map(|_| ());

    // allows for the following different terminations:
    // ' \n * */'
    // ' \n */'
    // '  */'
    let mut end = opt(line_end)
        .and(line_space0)
        .and(opt(sep.and(space)))
        .and(tag("*/"));

    move |input: &'a str| {
        let (input, _) = start.parse(input)?;
        let (input, res): (&'a str, Vec<Option<O>>) = separated_list0(
            |c| delimiter.parse(c),
            (|c| parser.parse(c))
                .map(Some)
                .or(inactive_comment.map(|_| None)),
        )(input)?;
        let (input, _) = cut(|c| end.parse(c)).parse(input)?;
        Ok((input, res.into_iter().filter_map(|c| c).collect()))
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
    use nom::sequence::delimited;

    use super::*;

    #[test]
    fn space_test() {
        assert_eq!(space("").unwrap().1, "");

        assert_eq!(space(" ").unwrap().1, " ");
        assert_eq!(space(" \tw").unwrap().1, " \t");
    }

    #[test]
    fn with_single_line_comment_test() {
        let mut parser = delimited(space, with_single_line_comment(tag("testing")), space);
        let test_str = r#"-- testing "#;
        assert!(parser.parse(test_str).unwrap().0 == "");
    }

    #[test]
    fn with_multi_line_comment_test() {
        let test_str = r#"
        /* testing */
"#;
        let mut parser = delimited(space, with_multi_line_comment(tag("testing")), space);
        assert_eq!(parser.parse(test_str).unwrap().1.len(), 1);

        let test_str = r#"
        /* testing
         *
         * not_testing
         * testing */
"#;
        assert_eq!(parser.parse(test_str).unwrap().1.len(), 2);

        let test_str = r#"
        /* testing
           testing */
"#;
        assert_eq!(parser.parse(test_str).unwrap().1.len(), 2);

        let test_str = r#"
        /* testing
         * testing
           testing 
         * testing 
         * */
"#;
        assert_eq!(parser.parse(test_str).unwrap().1.len(), 4);
    }

    #[test]
    fn separated_list_test() {
        let mut parser = separated_list0(tag(",").and(space), tag("t"));
        assert!(parser.parse("t, t").is_ok());
        assert!(parser.parse("t, t,").is_ok());
    }
}
