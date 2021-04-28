use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take, take_until, take_while},
    combinator::{eof, opt},
    multi::{fold_many0, many_till},
    sequence::{delimited, preceded, terminated},
    Err, IResult, Parser,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum ParseError<'a> {
    #[error("Parser failed at {0} due to {1:?}")]
    ParseError(&'a str, nom::error::ErrorKind),
    #[error("Parser failed at {0} due to {1}")]
    ConstError(&'a str, &'static str),
}

fn const_error<'a>(input: &'a str, reason: &'static str) -> ParseError<'a> {
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

type PResult<'a, O> = IResult<&'a str, O, ParseError<'a>>;

fn space(input: &str) -> PResult<&str> {
    opt(take_while(|chr: char| chr.is_whitespace()))(input)
        .map(|(input, val)| (input, val.unwrap_or("")))
}

fn dash_comment(input: &str) -> PResult<&str> {
    preceded(tag("--"), is_not("\n"))(input)
}

fn slash_comment(input: &str) -> PResult<Vec<&str>> {
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

fn decorator<'a, A, P>(decorator: &'static str, parser: P) -> impl FnMut(&'a str) -> PResult<A>
where
    P: Parser<&'a str, A, ParseError<'a>>,
{
    delimited(
        space.and(tag("@")).and(tag(decorator)).and(space),
        parser,
        opt(take_until("\n").and(take("\n".len()))),
    )
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Decorator<'a> {
    Endpoint(&'a str),
    Param(&'a str),
}

impl<'a> Decorator<'a> {
    fn parse_param(input: &'a str) -> PResult<&'a str> {
        decorator("param", take_while(|chr: char| chr.is_alphanumeric()))(input)
    }
    fn parse_endpoint(input: &'a str) -> PResult<&'a str> {
        decorator("endpoint", take_while(|chr: char| chr.is_alphanumeric()))(input)
    }

    pub fn parse(input: &'a str) -> PResult<Self> {
        alt((
            Self::parse_param.map(Decorator::Param),
            Self::parse_endpoint.map(Decorator::Endpoint),
        ))(input)
    }
}

fn frontmatter<'a>(input: &'a str) -> PResult<Vec<Decorator<'a>>> {
    enum Either<A> {
        Many(Vec<A>),
        One(A),
    }

    let (input, comments) = fold_many0(
        delimited(
            space,
            alt((
                dash_comment.map(Either::One),
                slash_comment.map(Either::Many),
            )),
            space,
        ),
        vec![],
        |mut acc, item: Either<&str>| match item {
            Either::Many(item) => {
                acc.extend(item.into_iter());
                acc
            }
            Either::One(item) => {
                acc.push(item);
                acc
            }
        },
    )(input)?;

    let decorators: Vec<Decorator<'a>> = comments
        .into_iter()
        .filter_map(|comment| Some(Decorator::parse(comment).ok()?.1))
        .collect();

    Ok((input, decorators))
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
                        let position = map.len();
                        map.insert(key, position);
                        position
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

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone)]
pub struct Module {
    pub endpoint: Option<String>,
    pub params: Vec<String>,
    pub sql: Vec<String>,
}

impl Module {
    fn normalize_sql_and_verify_params<'a>(
        input: &'a str,
        params_set: &BTreeSet<&str>,
    ) -> PResult<'a, String> {
        let (input, (sql, map)) = normalize_sql(input)?;
        if !map.keys().cloned().all(|val| params_set.contains(val)) {
            return Err(Err::Failure(const_error(
                input,
                "some used params are not declared",
            )));
        }
        Ok((input, sql))
    }

    pub fn parse(input: &str) -> PResult<Self> {
        let (input, decorators) = frontmatter(input)?;

        let mut endpoint = None;
        let mut params = vec![];
        let mut params_set = BTreeSet::new();

        for decorator in decorators.iter() {
            match *decorator {
                Decorator::Param(param) if params_set.contains(param) => {
                    Result::Err(Err::Failure(const_error(
                        input,
                        "multiple same parameters declarations detected",
                    )))?
                }
                Decorator::Param(param) => {
                    params.push(param.to_owned());
                    params_set.insert(param);
                }
                Decorator::Endpoint(dec) => match endpoint {
                    Some(_) => Result::Err(Err::Failure(const_error(
                        input,
                        "multiple endpoint declarations detected",
                    )))?,
                    None => {
                        endpoint = Some(dec.to_owned());
                    }
                },
            }
        }

        let (input, (statements, _)) = many_till(
            |input| Self::normalize_sql_and_verify_params(input, &params_set),
            eof,
        )(input)?;

        let module = Self {
            endpoint,
            sql: statements
                .into_iter()
                .filter(|stmt| stmt.as_str().trim() != "")
                .collect(),
            params: params.into_iter().map(String::from).collect(),
        };
        Ok((input, module))
    }
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
    fn decorator_parse_test() {
        let test_str = r#"@param shalom"#;
        assert_eq!(Decorator::parse_param(test_str).unwrap().1, "shalom");

        let test_str = "@endpoint getUsers \n\n";
        assert_eq!(Decorator::parse_endpoint(test_str).unwrap().1, "getUsers");
    }

    #[test]
    fn frontmatter_test() {
        let test_str = r#"
/* @endpoint getUser 
 * */
-- @param users
select * from users;
"#;
        assert_eq!(
            frontmatter(test_str).unwrap(),
            (
                "select * from users;\n",
                vec![Decorator::Endpoint("getUser"), Decorator::Param("users")]
            )
        );
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

    #[test]
    fn module_parse_test() {
        let test_str = r#"
-- @param email
-- @param id 
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id"#;
        let (_, module) = Module::parse(test_str).unwrap();
        assert_eq!(format!("{:?}", &module), "Module { endpoint: None, params: [\"email\", \"id\"], sql: [\"select * from users \\nwhere id = $0 \\nAND $1 = \\\'testing 123 @haha\\\' \\nOR 0 = $0\"] }");

        let test_str = r#"
/* @param email 
 * 
 */
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id"#;
        let err = Module::parse(test_str).unwrap_err();
        assert_eq!(
            format!("{:?}", &err),
            "Failure(ConstError(\"\", \"some used params are not declared\"))"
        );

        let test_str = r#"
/* @param email 
 * @param id
 */
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id;
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id ;
        "#;
        let err = Module::parse(test_str).unwrap().0;
        assert_eq!(err, "");

        let test_str = r#"
/* @param email 
 */
@email;test;;;test;
"#;
        let statements = Module::parse(test_str).unwrap().1.sql;
        assert_eq!(
            statements,
            vec!["$0".to_owned(), "test".to_owned(), "test".to_owned(),]
        );
    }
}
