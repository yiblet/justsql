use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take, take_while1},
    combinator::opt,
    multi::{fold_many0, fold_many1, separated_list1},
    sequence::{delimited, preceded},
    Parser,
};
use std::collections::BTreeSet;

use super::{
    module::{Interp, Statement},
    parser::{is_alpha_or_underscore, ErrorKind, PResult, ParseError},
};

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

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Param(&'a str),         // 'hello'
    AuthParam(&'a str),     // 'hello'
    StringLiteral(&'a str), // '" thing "'
    Word(&'a str),
    Space(&'a str),
    Other(char),
}

fn parse_token<'b, 'a: 'b>(
    params_set: &'b BTreeSet<&str>,
) -> impl FnMut(&'a str) -> PResult<'a, Token<'a>> + 'b {
    use Token::*;
    move |input: &'a str| {
        let auth_param = preceded(tag("@auth."), lex_word).map(AuthParam);

        let param = (move |og_input: &'a str| {
            let (input, word) = lex_at_word(og_input)?;
            if params_set.contains(word) {
                Ok((input, word))
            } else {
                Err(nom::Err::Failure(ParseError::error_kind(
                    og_input,
                    ErrorKind::UndefinedParameterError(word),
                )))?
            }
        })
        .map(Param);

        let string_literal = lex_string_literal.map(StringLiteral);
        let word = lex_word.map(Word);
        let space = lex_space.map(Space);
        let other = lex_other_char.map(Other);
        let (input, output) = alt((auth_param, param, string_literal, space, word, other))(input)?;
        Ok((input, output))
    }
}

fn parse_sql_statement<'b, 'a: 'b>(
    params_set: &'b BTreeSet<&str>,
) -> impl FnMut(&'a str) -> PResult<'a, Statement> + 'b {
    use Token::*;

    let mut parse_statement = fold_many1(
        parse_token(params_set),
        (String::new(), Vec::new()),
        |(mut builder, mut statement), token| match token {
            Param(param) => {
                if builder.len() != 0 {
                    statement.push(Interp::Literal(builder));
                    builder = String::new();
                }
                statement.push(Interp::Param(param.to_owned()));
                (builder, statement)
            }
            AuthParam(param) => {
                if builder.len() != 0 {
                    statement.push(Interp::Literal(builder));
                    builder = String::new();
                }
                statement.push(Interp::AuthParam(param.to_owned()));
                (builder, statement)
            }
            StringLiteral(lit) | Word(lit) | Space(lit) => {
                builder.push_str(lit);
                (builder, statement)
            }
            Other(chr) => {
                builder.push(chr);
                (builder, statement)
            }
        },
    )
    .map(|(final_literal, mut statement)| {
        if final_literal.len() == 0 {
            Statement(statement)
        } else {
            statement.push(Interp::Literal(final_literal));
            Statement(statement)
        }
    });

    move |input: &'a str| {
        let (input, statement) = parse_statement.parse(input)?;
        Ok((input, statement))
    }
}

pub fn parse_sql_statements<'b, 'a: 'b>(
    params_set: &'b BTreeSet<&str>,
) -> impl FnMut(&'a str) -> PResult<'a, Vec<Statement>> + 'b {
    move |og_input: &'a str| {
        let (input, res): (&str, Vec<Statement>) = separated_list1(
            fold_many1(lex_end_statement, (), |_, _| ()),
            parse_sql_statement(params_set),
        )(og_input)?;
        let (input, _) = opt(lex_end_statement)(input)?;

        let res: Vec<Statement> = res
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
        let set = ["id", "email"].iter().cloned().collect();
        let test_str = r#"select"#;
        let (_, token) = parse_token(&set)(test_str).unwrap();
        assert_eq!(token, Token::Word("select"));

        let test_str = r#"@id"#;
        let (_, token) = parse_token(&set)(test_str).unwrap();
        assert_eq!(token, Token::Param("id"));

        let test_str = r#"'testing'"#;
        let (_, token) = parse_token(&set)(test_str).unwrap();
        assert_eq!(token, Token::StringLiteral("'testing'"));
    }

    #[test]
    fn parse_sql_statement_test() {
        let set = ["id", "email"].iter().cloned().collect();
        let test_str =
            r#"select * from users where id = @id and @email = 'testing 123 @haha' OR 0 = @id"#;
        let (_, normalized_sql) = parse_sql_statement(&set)(test_str).unwrap();
        assert_eq!(
            format!("{:?}", normalized_sql),
            "Statement([Literal(\"select * from users where id = \"), Param(\"id\"), Literal(\" and \"), Param(\"email\"), Literal(\" = \\\'testing 123 @haha\\\' OR 0 = \"), Param(\"id\")])",
        );

        let test_str = r#"(@id)"#;
        let (_, normalized_sql) = parse_sql_statement(&set)(test_str).unwrap();
        assert_eq!(
            normalized_sql,
            Statement(vec![
                Interp::Literal("(".into()),
                Interp::Param("id".into()),
                Interp::Literal(")".into())
            ])
        );
    }

    #[test]
    fn parse_sql_statements_test() {
        let set = ["id", "email"].iter().cloned().collect();
        let test_str = r#"
            select * from users where id = @id and @email = 'testing 123 @haha' OR 0 = @id;
            select * from users;;
            select * from users;
        "#;
        let (_, normalized_sql) = parse_sql_statements(&set)(test_str).unwrap();
        assert_eq!(normalized_sql.len(), 3);

        let test_str = r#"
        ;;; ;
        "#;
        let _err = parse_sql_statements(&set)(test_str).unwrap_err();
    }
}
