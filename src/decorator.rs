use nom::{
    branch::alt,
    bytes::complete::{tag, take, take_until, take_while},
    combinator::opt,
    multi::fold_many0,
    sequence::delimited,
    Parser,
};

use crate::parser::{dash_comment, slash_comment, space, PResult, ParseError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decorator<'a> {
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

pub fn frontmatter<'a>(input: &'a str) -> PResult<Vec<Decorator<'a>>> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
