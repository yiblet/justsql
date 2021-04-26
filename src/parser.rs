use nom::{
    branch::alt,
    bytes::complete::{escaped, is_a, is_not, tag, take_while},
    character::complete::one_of,
    combinator::opt,
    error::ParseError,
    multi::{many1, separated_list0, separated_list1},
    sequence::{delimited, preceded, terminated, tuple},
    Compare, IResult, InputLength, InputTake, InputTakeAtPosition, Parser,
};

enum Key {
    // .string | ['string']
    Member(String),
    // [2]
    Idx(i64),
}

enum Ast {
    // .string | ['string'] | [2]
    KeyChain(Vec<Key>),
    // [ <ast>, <ast> ],
    TupleMap(Vec<Ast>),
    // { string: <ast>, 'key': <ast> },
    DictMap(Vec<(String, Ast)>),
}

fn parse_ast(input: &str) -> IResult<&str, Ast> {
    alt((
        many1(parse_key).map(Ast::KeyChain),
        parse_tuple_map.map(Ast::TupleMap),
        parse_dict_map.map(Ast::DictMap),
    ))(input)
}

fn parse_member(input: &str) -> IResult<&str, String> {
    alt((array_member, dot_member))
        .map(str::to_owned)
        .parse(input)
}

fn parse_idx(input: &str) -> IResult<&str, i64> {
    array_delimited(integer)(input)
}

fn parse_key(input: &str) -> IResult<&str, Key> {
    alt((parse_member.map(Key::Member), parse_idx.map(Key::Idx)))(input)
}

// <ast>, <ast>
fn parse_tuple_map(input: &str) -> IResult<&str, Vec<Ast>> {
    array_delimited(terminated(
        separated_list0(comma_separator, parse_ast),
        opt(comma_separator),
    ))(input)
}

fn parse_dict_map(input: &str) -> IResult<&str, Vec<(String, Ast)>> {
    fn parse_mapping(input: &str) -> IResult<&str, (String, Ast)> {
        let mut dict_key = alt((string_constant, is_not(" \t\n\r:")));
        let (input, key) = dict_key(input)?;
        let (input, _) = tuple((space, tag(":"), space))(input)?;
        parse_ast.map(|ast| (key.to_string(), ast)).parse(input)
    }

    // { string: <ast>, string: <ast>, }
    delimited(
        tuple((tag("{"), space)),
        terminated(
            separated_list0(comma_separator, parse_mapping),
            opt(comma_separator),
        ),
        tuple((space, tag("}"))),
    )(input)
}

fn comma_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = terminated(tag(","), space)(input)?;
    Ok((input, ()))
}

fn integer(input: &str) -> IResult<&str, i64> {
    fn binary(input: &str) -> IResult<&str, i64> {
        let (input, text) = preceded(alt((tag("0b"), tag("0B"))), is_a("01"))(input)?;
        let mut sum: i64 = 0;
        for chr in text.chars() {
            sum *= 2;
            if chr == '1' {
                sum += 1
            }
        }
        Ok((input, sum))
    }

    fn hex(input: &str) -> IResult<&str, i64> {
        let (input, text) =
            preceded(alt((tag("0x"), tag("0X"))), is_a("01234567890ABCDEFabcdef"))(input)?;
        let mut sum: i64 = 0;
        for chr in text.chars().map(|chr: char| chr.to_ascii_lowercase()) {
            sum *= 16;
            match chr {
                '0'..='9' => sum += (chr as i64) - ('0' as i64),
                'a' => sum += 10,
                'b' => sum += 11,
                'c' => sum += 12,
                'd' => sum += 13,
                'e' => sum += 14,
                'f' => sum += 15,
                _ => panic!("no other character should be possible"),
            }
            println!("sum")
        }
        Ok((input, sum))
    }

    fn decimal(input: &str) -> IResult<&str, i64> {
        let (input, binary) = is_a("01234567890")(input)?;
        let mut sum: i64 = 0;
        for chr in binary.chars().map(|chr: char| chr.to_ascii_lowercase()) {
            sum *= 10;
            sum += (chr as i64) - ('0' as i64)
        }
        Ok((input, sum))
    }

    let (input, minus) = opt(tag("-"))(input)?;
    let (input, _) = opt(space)(input)?;
    let (output, number) = alt((hex, binary, decimal))(input)?;
    Ok((output, if minus.is_some() { -number } else { number }))
}

// '.string' -> 'string'
fn dot_member(input: &str) -> IResult<&str, &str> {
    let (input, _) = tag(".")(input)?;
    let (input, key) = is_not(".\"'\t \n\r<{[()]}>")(input)?;

    Ok((input, key))
}

// TODO include all js escape strings
fn string_constant(input: &str) -> IResult<&str, &str> {
    alt((
        delimited(
            tag("'"),
            escaped(is_not("\\'"), '\\', one_of("'")),
            tag("'"),
        ),
        delimited(
            tag("\""),
            escaped(is_not("\"\\"), '\\', one_of("\"")),
            tag("\""),
        ),
    ))(input)
}

fn space<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&str, &str, E> {
    is_a("\t\n\r ")(input)
}

fn array_member(input: &str) -> IResult<&str, &str> {
    array_delimited(string_constant)(input)
}

fn array_delimited<'a, O, E: ParseError<&'a str>, F>(
    mut first: F,
) -> impl FnMut(&'a str) -> IResult<&str, O, E>
where
    F: Parser<&'a str, O, E>,
{
    move |input: &'a str| {
        delimited(
            terminated(tag("["), space),
            |input| first.parse(input),
            preceded(space, tag("]")),
        )(input)
    }
}

#[cfg(test)]
mod tests {
    use nom::combinator::all_consuming;

    use super::*;

    #[test]
    fn literal_test() {
        let val = dot_member(".testing");
        assert_eq!(val.unwrap().1, "testing");

        let val = dot_member(".testing ");
        assert_eq!(val.unwrap().1, "testing");

        let val = dot_member(".testing");
        assert_eq!(val.unwrap().1, "testing");
    }

    #[test]
    fn string_constant_test() {
        let val: IResult<&str, &str> = is_not("\\\"")(r#"testing"#);
        assert_eq!(val.unwrap().1, "testing");

        let val = string_constant(r#""testing\"t""#);
        assert_eq!(val.unwrap().1, r#"testing\"t"#);
    }

    #[test]
    fn number_test() {
        let val: IResult<&str, i64> = integer("123");
        assert_eq!(val.unwrap().1, 123);

        let val: IResult<&str, i64> = integer("0x10");
        assert_eq!(val.unwrap().1, 0x10);

        let val: IResult<&str, i64> = integer("0b1011");
        assert_eq!(val.unwrap().1, 0b1011);

        let val: IResult<&str, i64> = integer("-123");
        assert_eq!(val.unwrap().1, -123);

        let val: IResult<&str, i64> = integer("- 123");
        assert_eq!(val.unwrap().1, -123);
    }
}
