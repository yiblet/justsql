use nom::{
    branch::alt,
    bytes::complete::{tag, take, take_until, take_while},
    character::complete::one_of,
    combinator::opt,
    multi::fold_many0,
    number::complete::float,
    sequence::{delimited, preceded},
    Parser,
};

use crate::{
    module::AuthSettings,
    parser::{const_error, dash_comment, slash_comment, space, PResult, ParseError},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Decorator<'a> {
    Endpoint(&'a str),
    Param(&'a str),
    Auth(AuthSettings),
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
                * get_multiplier(chr).map_err(|err| nom::Err::Failure(const_error(input, err)))?
        }
    };
    Ok((output, seconds))
}

impl<'a> Decorator<'a> {
    fn parse_param(input: &'a str) -> PResult<&'a str> {
        decorator("param", take_while(|chr: char| chr.is_alphanumeric()))(input)
    }

    fn parse_endpoint(input: &'a str) -> PResult<&'a str> {
        decorator("endpoint", take_while(|chr: char| chr.is_alphanumeric()))(input)
    }

    fn parse_auth(input: &'a str) -> PResult<AuthSettings> {
        let verify_token = preceded(tag("verify").and(space), opt(parse_interval))
            .map(|opt| opt.map(|val| val as u64))
            .map(AuthSettings::VerifyToken);

        let set_token = preceded(tag("authorize").and(space), parse_interval)
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

        let test_str = "@auth verify \n\n";
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
