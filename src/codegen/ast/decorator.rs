use either::Either;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while},
    character::complete::one_of,
    combinator::{cut, opt},
    multi::fold_many0,
    number::complete::float,
    sequence::{delimited, preceded},
    Parser,
};
use std::path::{Path, PathBuf};

use crate::codegen::module::AuthSettings;

use super::{
    super::result::{PResult, ParseError},
    super::span_ref::SpanRef,
    parser::{
        is_alpha_or_underscore, line_space0, line_space1, space, string_literal,
        with_multi_line_comment, with_single_line_comment,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub enum Decorator<'a> {
    Auth(AuthSettings),
    Import(SpanRef<'a, &'a str>, SpanRef<'a, &'a Path>),
    Endpoint(&'a str),
    Param(&'a str),
}

fn get_multiplier(chr: char) -> Result<f32, &'static str> {
    let res = match chr {
        's' => 1f32,
        'm' => 60f32,
        'h' => 60f32 * 60f32,
        'd' => 60f32 * 60f32 * 24f32,
        'M' => 60f32 * 60f32 * 24f32 * 30f32,
        'y' => 60f32 * 60f32 * 24f32 * 365f32,
        _ => Err("invalid time multiplier")?,
    };
    Ok(res)
}

fn parse_interval(input: &str) -> PResult<f32> {
    let (output, (seconds, chr_opt)) = float.and(opt(one_of("smhdMy"))).parse(input)?;
    let seconds = match chr_opt {
        None => seconds,
        Some(chr) => {
            seconds
                * get_multiplier(chr)
                    .map_err(|err| nom::Err::Failure(ParseError::const_error(input, err)))?
        }
    };
    Ok((output, seconds))
}

impl<'a> Decorator<'a> {
    fn parse_param(input: &'a str) -> PResult<&'a str> {
        decorator("param", take_while(is_alpha_or_underscore))(input)
    }

    fn parse_import(input: &'a str) -> PResult<(SpanRef<'a, &'a str>, SpanRef<'a, &'a Path>)> {
        let import = |input: &'a str| {
            let (input, import_name) = SpanRef::parse(take_while(is_alpha_or_underscore))(input)?;
            let (input, _) = line_space1(input)?;
            let (input, _) = tag("from")(input)?;
            let (input, _) = line_space1(input)?;
            let (input, literal) = SpanRef::parse(string_literal)(input)?;

            if literal.len() < 3 {
                Err(nom::Err::Failure(ParseError::const_error(
                    literal.start,
                    "invalid relative path",
                )))?
            };

            let path = literal.map(|path| Path::new(&path[1..path.len() - 1]));

            if !path.is_relative() {
                Err(nom::Err::Failure(ParseError::const_error(
                    literal.start,
                    "path is not a valid relative path",
                )))?
            }

            Ok((input, (import_name, path)))
        };
        decorator("import", import)(input)
    }

    fn parse_endpoint(input: &'a str) -> PResult<&'a str> {
        decorator("endpoint", take_while(is_alpha_or_underscore))(input)
    }

    fn parse_auth(input: &'a str) -> PResult<AuthSettings> {
        let verify_token = preceded(tag("verify"), opt(preceded(line_space0, parse_interval)))
            .map(|opt| opt.map(|val| val as u64))
            .map(AuthSettings::VerifyToken);

        let set_token = preceded(tag("authorize").and(line_space1), parse_interval)
            .map(|val| val as u64)
            .map(AuthSettings::SetToken);

        let remove_token = tag("clear").map(|_| AuthSettings::RemoveToken);

        decorator("auth", alt((verify_token, set_token, remove_token)))(input)
    }

    pub fn parse(input: &'a str) -> PResult<Self> {
        alt((
            Self::parse_param.map(Decorator::Param),
            Self::parse_endpoint.map(Decorator::Endpoint),
            Self::parse_auth.map(Decorator::Auth),
            Self::parse_import.map(|(v1, v2)| Decorator::Import(v1, v2)),
        ))(input)
    }
}

fn decorator<'a, A, P>(decorator: &'static str, parser: P) -> impl FnMut(&'a str) -> PResult<A>
where
    P: Parser<&'a str, A, ParseError<'a>>,
{
    delimited(
        line_space0
            .and(tag("@"))
            .and(tag(decorator))
            .and(line_space1),
        cut(parser),
        line_space0,
    )
}

#[derive(Debug, Clone)]
pub struct Decorators<'a>(pub Vec<SpanRef<'a, Decorator<'a>>>);

impl<'a> std::ops::Deref for Decorators<'a> {
    type Target = Vec<SpanRef<'a, Decorator<'a>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> Decorators<'a> {
    pub fn into_inner(self) -> Vec<SpanRef<'a, Decorator<'a>>> {
        self.0
    }

    pub fn canonicalized_dependencies<'b>(
        &'b self,
        file_loc: &'b Path,
    ) -> impl Iterator<Item = SpanRef<'a, PathBuf>> + '_ {
        self.dependencies(file_loc)
            .filter_map(|dep| dep.with(dep.canonicalize()).transpose().ok())
    }

    pub fn dependencies<'b>(
        &'b self,
        file_loc: &'b Path,
    ) -> impl Iterator<Item = SpanRef<'a, PathBuf>> + 'b {
        self.0
            .iter()
            .filter_map(move |decorator| match &decorator.value {
                Decorator::Import(_, path) => path
                    .map(|path| {
                        let mut cur_loc = file_loc.to_path_buf();
                        cur_loc.pop();
                        cur_loc.push(path);
                        Some(cur_loc)
                    })
                    .transpose(),
                _ => None,
            })
    }

    // TODO do not permit decorators with stuff after that isn't a space
    pub fn parse(input: &'a str) -> PResult<Self> {
        let (input, decorators) = fold_many0(
            delimited(
                space,
                alt((
                    with_multi_line_comment(SpanRef::<Decorator>::parse(Decorator::parse))
                        .map(Either::Left),
                    with_single_line_comment(SpanRef::<Decorator>::parse(Decorator::parse))
                        .map(Either::Right),
                )),
                space,
            ),
            vec![],
            |mut acc, item| match item {
                Either::Left(item) => {
                    acc.extend(item.into_iter());
                    acc
                }
                Either::Right(Some(item)) => {
                    acc.push(item);
                    acc
                }
                Either::Right(None) => acc,
            },
        )(input)?;

        Ok((input, Self(decorators)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorator_parse_test() {
        let test_str = r#"@param shalom_yiblet"#;
        assert_eq!(Decorator::parse_param(test_str).unwrap().1, "shalom_yiblet");

        let test_str = r#"@param shalom"#;
        assert_eq!(Decorator::parse_param(test_str).unwrap().1, "shalom");

        let test_str = "@endpoint getUsers \n\n";
        assert_eq!(Decorator::parse_endpoint(test_str).unwrap().1, "getUsers");

        let test_str = "@auth verify \n\n";
        assert_eq!(
            Decorator::parse_auth(test_str).unwrap().1,
            AuthSettings::VerifyToken(None)
        );

        let test_str = "@auth verify";
        assert_eq!(
            Decorator::parse_auth(test_str).unwrap().1,
            AuthSettings::VerifyToken(None)
        );

        let test_str = "@auth verify 2d \n\n";
        assert_eq!(
            Decorator::parse_auth(test_str).unwrap().1,
            AuthSettings::VerifyToken(Some(60 * 60 * 24 * 2))
        );

        let test_str = "@auth authorize 32d \n\n";
        assert_eq!(
            Decorator::parse_auth(test_str).unwrap().1,
            AuthSettings::SetToken(60 * 60 * 24 * 32)
        );
    }

    #[test]
    fn input_decorator_test() {
        fn unwrap_spans<A, B>((v1, v2): (SpanRef<A>, SpanRef<B>)) -> (A, B) {
            (v1.value, v2.value)
        }
        let test_str = "@import friends_of from './../friends' \n\n";
        assert_eq!(
            unwrap_spans(Decorator::parse_import(test_str).unwrap().1),
            ("friends_of", Path::new("./../friends"))
        );

        let test_str = "@import friends_of from 'friends' \n\n";
        assert_eq!(
            unwrap_spans(Decorator::parse_import(test_str).unwrap().1),
            ("friends_of", Path::new("friends"))
        );

        let test_str = "@import friends_of from '/friends' \n\n";
        assert!(Decorator::parse_import(test_str).is_err());

        let test_str = "@import friends_@of from './friends' \n\n";
        assert!(Decorator::parse_import(test_str).is_err());
    }

    fn parse_decorators(input: &str) -> PResult<Vec<SpanRef<'_, Decorator<'_>>>> {
        Decorators::parse.map(|v| v.0).parse(input)
    }

    #[test]
    fn parse_decorators_test() {
        fn unwrap<'a>(vec: Vec<SpanRef<'a, Decorator<'a>>>) -> Vec<Decorator<'a>> {
            vec.into_iter().map(|span| span.value).collect()
        }

        let test_str = r#"
/* @endpoint getUser 
 * */
-- @param users
select * from users;
"#;
        assert_eq!(
            parse_decorators.map(unwrap).parse(test_str).unwrap(),
            (
                "select * from users;\n",
                vec![Decorator::Endpoint("getUser"), Decorator::Param("users")]
            )
        );

        let test_str = r#"
-- testing 
-- @param testing
-- testing
-- @param users
-- @param testing testing
select * from users;
"#;
        assert!(parse_decorators.map(unwrap).parse(test_str).is_err(),);

        let test_str = r#"
/* @endpoint getUser 
 * @param users */
select * from users;
"#;
        assert_eq!(
            parse_decorators.map(unwrap).parse(test_str).unwrap(),
            (
                "select * from users;\n",
                vec![Decorator::Endpoint("getUser"), Decorator::Param("users")]
            )
        );

        let test_str = r#"
/* @endpoint getUser 
 * @param users users
 * user */
select * from users;
"#;
        let err = parse_decorators(test_str).unwrap_err();
        assert!(match err {
            nom::Err::Failure(ParseError::NomError(v, _)) => v.starts_with("users\n"),
            _ => panic!("{}", err),
        });

        let test_str = r#"
-- @auth verify
-- @auth verify 2d
-- @param users
select * from users;
"#;
        let (_, decs) = parse_decorators(test_str).unwrap();
        assert_eq!(decs.len(), 3);

        let test_str = r#"
-- @auth vxerify
-- @param users
select * from users;
"#;
        assert!(parse_decorators(test_str).is_err());
    }
}
