use std::collections::BTreeMap;

use anyhow::anyhow;

fn parse_arg(input: &str) -> anyhow::Result<(&str, &str)> {
    let (prefix, suffix) = input.split_at(
        input
            .find('=')
            .ok_or_else(|| anyhow!("arguments must be of the form key=value"))?,
    );
    Ok((prefix.trim(), &suffix[1..].trim()))
}

pub type Args<'a> = BTreeMap<&'a str, Literal>;

pub fn parse_args<'a, I: Iterator<Item = &'a str>>(
    input: I,
) -> anyhow::Result<BTreeMap<&'a str, Literal>> {
    let mut res = BTreeMap::new();
    for arg in input {
        let (key, value) = parse_arg(arg)?;
        if let Some(_) = res.insert(key, Literal::parse(value)?) {
            Err(anyhow!("duplicate argument {}", key))?;
        }
    }

    Ok(res)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    String(String),
    Float(f64),
}

impl Literal {
    /// parses literals
    /// TODO add tests for parsing
    fn parse(input: &str) -> anyhow::Result<Literal> {
        fn is_digit(input: &str) -> bool {
            input.trim().chars().all(|chr| chr.is_digit(10))
        }
        if is_digit(input) {
            return Ok(Literal::Int(input.parse()?));
        };
        let split: Vec<&str> = input.trim().split('.').collect();
        if split.len() == 2 && is_digit(split[0]) && is_digit(split[1]) {
            return Ok(Literal::Float(input.parse()?));
        }
        if split.len() != 2
            && input.len() > 2
            && &input[0..1] == "'"
            && &input[input.len() - 1..] == "'"
        {
            return Ok(Literal::String(format!("'{}'", &input[1..input.len() - 1])));
        }
        Err(anyhow!("could not infer literal"))
    }

    /// converts literal to string
    pub fn to_string(&self) -> String {
        match self {
            Literal::Int(v) => format!("{}", v),
            Literal::Float(v) => format!("{}", v),
            Literal::String(literal) => format!("'{}'", literal),
        }
    }
}

