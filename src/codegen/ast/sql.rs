use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    combinator::opt,
    multi::{fold_many1, separated_list0, separated_list1},
    sequence::{delimited, preceded, terminated},
    Parser,
};

use super::{
    super::result::{ErrorKind, PResult, ParseError},
    super::span_ref::SpanRef,
    parser::{is_alpha_or_underscore, space, string_literal},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSpan<'a>(pub Vec<SpanRef<'a, InterpSpan<'a>>>);

impl<'a> StatementSpan<'a> {
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|interp| match &interp.value {
            // all literals are pure whitespace
            InterpSpan::Literal(lit) => lit.find(|chr: char| !chr.is_whitespace()).is_none(),

            // if using a call site then the statement is nonempty
            InterpSpan::CallSite(_, _) => true,

            // other types of interps do not exist
            _ => false,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpSpan<'a> {
    Literal(String), // literals are parsed combined together
    Param(&'a str),
    AuthParam(&'a str),
    CallSite(&'a str, Vec<SpanRef<'a, &'a str>>),
}

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Param(&'a str),                               // 'hello'
    AuthParam(&'a str),                           // 'hello'
    CallSite(&'a str, Vec<SpanRef<'a, &'a str>>), // 'hello'
    StringLiteral(&'a str),                       // '" thing "'
    Word(&'a str),
    Space(&'a str),
    Other(char),
}

fn lex_word<'a>(input: &'a str) -> PResult<'a, &'a str> {
    take_while1(is_alpha_or_underscore)(input)
}

fn lex_at_word<'a>(input: &'a str) -> PResult<'a, &'a str> {
    preceded(
        nom::character::complete::char('@'),
        take_while1(is_alpha_or_underscore),
    )
    .parse(input)
}

fn lex_string_literal<'a>(input: &'a str) -> PResult<'a, &'a str> {
    string_literal(input)
}

fn lex_end_statement<'a>(input: &'a str) -> PResult<'a, ()> {
    nom::character::complete::char(';').map(|_| ()).parse(input)
}

fn lex_space<'a>(input: &'a str) -> PResult<'a, &'a str> {
    let loc = input.find(|chr: char| !chr.is_whitespace());
    match loc {
        Some(0) | None => Err(nom::Err::Error(ParseError::const_error(
            input,
            "expected space",
        ))),
        Some(pos) => {
            let (space, rest) = input.split_at(pos);
            Ok((rest, space))
        }
    }
}

fn lex_other_char<'a>(input: &'a str) -> PResult<'a, char> {
    nom::character::complete::satisfy(|c| c != ';')(input)
}

fn parse_token<'a>(input: &'a str) -> PResult<'a, Token<'a>> {
    {
        use Token::*;
        let auth_param = preceded(tag("@auth."), lex_word).map(AuthParam);
        let param = lex_at_word.map(Param);
        let call_site = lex_at_word
            .and(delimited(
                tag("("),
                terminated(
                    separated_list0(space.and(tag(",")).and(space), |input: &'a str| {
                        let (input, res) = SpanRef::parse(lex_word)(input)?;
                        Ok((input, res))
                    }),
                    opt(space.and(tag(",")).and(space)),
                ),
                tag(")"),
            ))
            .map(|(func, params): (&'a str, Vec<SpanRef<'a, &'a str>>)| CallSite(func, params));
        let string_literal = lex_string_literal.map(StringLiteral);
        let word = lex_word.map(Word);
        let space = lex_space.map(Space);
        let other = lex_other_char.map(Other);
        let (input, output) = alt((
            call_site,
            auth_param,
            param,
            string_literal,
            space,
            word,
            other,
        ))(input)?;
        Ok((input, output))
    }
}

fn parse_sql_statement<'a>(input: &'a str) -> PResult<'a, StatementSpan<'a>> {
    use Token::*;

    let parse_token = |input: &'a str| {
        let (input, token) = SpanRef::parse(parse_token)(input)?;
        Ok((input, token))
    };

    let mut parse_statement = fold_many1(
        parse_token,
        (
            SpanRef {
                start: input,
                end: input,
                value: String::new(),
            },
            Vec::new(),
        ),
        |(mut builder, mut statement), token: SpanRef<'a, Token>| {
            // first set builder
            match &token.value {
                Param(_) | AuthParam(_) | CallSite(_, _) => {
                    if builder.len() != 0 {
                        statement.push(builder.map(InterpSpan::Literal));
                        builder = SpanRef {
                            start: token.end,
                            end: token.end,
                            value: String::new(),
                        };
                    }
                }
                StringLiteral(lit) | Word(lit) | Space(lit) => {
                    builder.push_str(lit);
                }
                Other(chr) => {
                    builder.push(*chr);
                }
            };

            // second add the current parameter
            match &token.value {
                Param(param) => {
                    statement.push(token.as_ref().map(|_| InterpSpan::Param(param)));
                }
                AuthParam(param) => {
                    statement.push(token.as_ref().map(|_| InterpSpan::AuthParam(param)));
                }
                CallSite(func, args) => {
                    statement.push(
                        token
                            .as_ref()
                            .map(|_| InterpSpan::CallSite(func, args.clone())),
                    );
                }
                _ => {}
            };

            (builder, statement)
        },
    )
    .map(|(final_literal, mut statement)| {
        let statement_span = if final_literal.len() == 0 {
            statement
        } else {
            statement.push(final_literal.map(InterpSpan::Literal));
            statement
        };
        StatementSpan(statement_span)
    });

    let (input, statement) =
        parse_statement
            .parse(input)
            .map_err(|err: nom::Err<ParseError>| {
                err.map(|err| match err {
                    ParseError::NomError(input, nom::error::ErrorKind::Many1) => {
                        ParseError::const_error(input, "statement(s) are empty")
                    }
                    _ => err,
                })
            })?;
    Ok((input, statement))
}

pub fn parse_statements<'a>(og_input: &'a str) -> PResult<'a, Vec<SpanRef<'a, StatementSpan<'a>>>> {
    let (input, res): (&str, Vec<SpanRef<StatementSpan>>) = separated_list1(
        fold_many1(lex_end_statement, (), |_, _| ()),
        SpanRef::<StatementSpan>::parse(parse_sql_statement),
    )(og_input)?;
    let (input, _) = opt(lex_end_statement)(input)?;

    let res: Vec<SpanRef<StatementSpan>> = res
        .into_iter()
        .filter(|statement| !statement.is_empty())
        .collect();

    if res.len() == 0 {
        return Err(nom::Err::Failure(ParseError::error_kind(
            og_input,
            ErrorKind::ConstError("this module has no statements"),
        )));
    }

    Ok((input, res))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn string_literal_test() {
        let test_str = r#""test" "#;
        assert_eq!(string_literal(test_str).unwrap(), (" ", r#""test""#));
    }

    #[test]
    fn parse_token_test() {
        let test_str = r#"select"#;
        let (_, token) = parse_token(test_str).unwrap();
        assert_eq!(token, Token::Word("select"));

        let test_str = r#"@id"#;
        let (_, token) = parse_token(test_str).unwrap();
        assert_eq!(token, Token::Param("id"));

        let test_str = r#"@func(id, b)"#;
        let (_, token) = parse_token(test_str).unwrap();

        let call_site = crate::matches_map!(token,
            Token::CallSite("func", vals) => vals.iter().map(|span| span.value).collect::<Vec<_>>()
        );
        assert_eq!(call_site, Some(vec!["id", "b"]));

        let test_str = r#"@func(id, b, c)"#;
        let (_, token) = parse_token(test_str).unwrap();
        let call_site = crate::matches_map!(token,
            Token::CallSite("func", vals) => vals.iter().map(|span| span.value).collect::<Vec<_>>()
        );
        assert_eq!(call_site, Some(vec!["id", "b", "c"]));

        let test_str = r#"'testing'"#;
        let (_, token) = parse_token(test_str).unwrap();
        assert_eq!(token, Token::StringLiteral("'testing'"));
    }

    #[test]
    fn parse_sql_statement_test() {
        let test_str =
            r#"select * from users where id = @id and @email = 'testing 123 @haha' OR 0 = @id"#;
        let (_, normalized_sql) = parse_sql_statement
            .map(|stmt| {
                stmt.0
                    .into_iter()
                    .map(|span| span.value)
                    .collect::<Vec<_>>()
            })
            .parse(test_str)
            .unwrap();
        assert_eq!(
            format!("{:?}", normalized_sql),
            "[Literal(\"select * from users where id = \"), Param(\"id\"), Literal(\" and \"), Param(\"email\"), Literal(\" = \\\'testing 123 @haha\\\' OR 0 = \"), Param(\"id\")]",
        );

        let test_str = r#"(@id)"#;
        let (_, normalized_sql) = parse_sql_statement
            .map(|stmt| {
                stmt.0
                    .into_iter()
                    .map(|span| span.value)
                    .collect::<Vec<_>>()
            })
            .parse(test_str)
            .unwrap();
        assert_eq!(
            normalized_sql,
            vec![
                InterpSpan::Literal("(".into()),
                InterpSpan::Param("id"),
                InterpSpan::Literal(")".into())
            ]
        );
    }

    #[test]
    fn parse_sql_statements_test() {
        let test_str = r#"
            select * from users where id = @id and @email = 'testing 123 @haha' OR 0 = @id;
            select * from users;;
            select * from users;
        "#;
        let (_, normalized_sql) = parse_statements(test_str).unwrap();
        assert_eq!(normalized_sql.len(), 3);

        let test_str = r#"
        ;;; ;
        "#;
        let _err = parse_statements(test_str).unwrap_err();
    }
}
