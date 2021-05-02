use crate::{
    ast::{decorator::Decorator, parser::PResult},
    binding::Binding,
    server::auth::decode,
};
use nom::Err;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::Path,
};
use thiserror::Error;

use super::{parser::ParseError, sql::parse_sql_statements};

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone, PartialEq)]
pub enum AuthSettings {
    VerifyToken(Option<u64>),
    SetToken(u64), // number of seconds till expiration
    RemoveToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interp {
    Literal(String),
    Param(String),
    AuthParam(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Statement(pub Vec<Interp>);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub enum ParamType<'a> {
    Auth(&'a str),
    Param(&'a str),
}

impl Statement {
    /// identifies empty statements
    /// i.e. the statement '; ;'
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|interp| match interp {
            // all literals are pure whitespace
            Interp::Literal(lit) => lit.find(|chr: char| !chr.is_whitespace()).is_none(),
            // other types of interps do not exist
            _ => false,
        })
    }

    pub fn bind(&self) -> anyhow::Result<(String, Vec<ParamType>)> {
        let mut params = vec![];
        let mut mapping: BTreeMap<ParamType, usize> = BTreeMap::new();
        let mut res = String::new();
        for interp in &self.0 {
            match interp {
                Interp::Literal(lit) => write!(&mut res, "{}", lit.as_str())?,
                Interp::AuthParam(param)
                    if mapping.contains_key(&ParamType::Auth(param.as_str())) =>
                {
                    write!(&mut res, "${}", mapping[&ParamType::Auth(param.as_str())])?
                }
                Interp::AuthParam(param) => {
                    let cur = mapping.len() + 1;
                    let param = ParamType::Auth(param);
                    mapping.insert(param, cur);
                    params.push(param);
                    write!(&mut res, "${}", cur)?
                }
                Interp::Param(param) if mapping.contains_key(&ParamType::Param(param.as_str())) => {
                    write!(&mut res, "${}", mapping[&ParamType::Param(param.as_str())])?
                }
                Interp::Param(param) => {
                    let cur = mapping.len() + 1;
                    let param = ParamType::Param(param);
                    mapping.insert(param, cur);
                    params.push(param);
                    write!(&mut res, "${}", cur)?
                }
            }
        }
        Ok((res, params))
    }
}

#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{error}")]
    ParseError {
        file: String,
        pos: usize,
        error: String,
    },
    #[error("unexpected token")]
    NomParseError { file: String, pos: usize },
    #[error("file is incomplete")]
    Incomplete,
}

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone, Default)]
pub struct Module {
    pub auth: Option<AuthSettings>,
    pub endpoint: Option<String>,
    pub params: Vec<String>,
    pub sql: Vec<Statement>,
}

impl Module {
    pub fn verify(
        &self,
        cookie: Option<&str>,
    ) -> anyhow::Result<Option<BTreeMap<String, Binding>>> {
        if matches!(self.auth, Some(AuthSettings::VerifyToken(_))) {
            return decode(cookie.ok_or_else(|| anyhow!("missing cookie"))?)
                .map(|claim| Some(claim.claims));
        }
        Ok(None)
    }

    pub fn from_path<A: AsRef<Path>>(input: A) -> Result<Module, ModuleError> {
        use std::io::prelude::*;
        let path = input.as_ref();
        let mut file = std::fs::File::open(path)?;
        let mut file_content = String::with_capacity(file.metadata()?.len() as usize);
        file.read_to_string(&mut file_content)?;

        return match Module::parse(file_content.as_str()) {
            Ok((_, res)) => Ok(res),
            Err(nom::Err::Incomplete(_)) => Err(ModuleError::Incomplete),
            Err(nom::Err::Failure(ParseError::NomError(input, _)))
            | Err(nom::Err::Error(ParseError::NomError(input, _))) => {
                let pos = file_content.len() - input.len();
                Err(ModuleError::NomParseError {
                    file: file_content,
                    pos,
                })
            }
            Err(nom::Err::Failure(ParseError::ErrorKind(input, kind)))
            | Err(nom::Err::Error(ParseError::ErrorKind(input, kind))) => {
                let pos = file_content.len() - input.len();
                let error = format!("{}", kind);
                Err(ModuleError::ParseError {
                    file: file_content,
                    pos,
                    error,
                })
            }
        };
    }

    pub fn parse(input: &str) -> PResult<Self> {
        let (input, decorators) = crate::ast::decorator::frontmatter(input)?;

        let mut endpoint = None;
        let mut params = vec![];
        let mut params_set = BTreeSet::new();
        let mut auth_settings = None;

        for decorator in decorators.into_iter() {
            match decorator {
                Decorator::Auth(_) if auth_settings.is_some() => Result::Err(Err::Failure(
                    ParseError::const_error(input, "multiple auth declarations detected"),
                ))?,
                Decorator::Auth(val) => auth_settings = Some(val),
                Decorator::Param(param) if params_set.contains(param) => {
                    Result::Err(Err::Failure(ParseError::const_error(
                        input,
                        "multiple same parameters declarations detected",
                    )))?
                }
                Decorator::Param(param) => {
                    params.push(param.to_owned());
                    params_set.insert(param);
                }
                Decorator::Endpoint(dec) => match endpoint {
                    Some(_) => Result::Err(Err::Failure(ParseError::const_error(
                        input,
                        "multiple endpoint declarations detected",
                    )))?,
                    None => {
                        endpoint = Some(dec.to_owned());
                    }
                },
            }
        }

        let (input, sql) = parse_sql_statements(&params_set)(input)?;

        if input != "" {
            Err(nom::Err::Failure(ParseError::const_error(
                input,
                "unexpected token",
            )))?
        }

        let has_auth = sql
            .iter()
            .flat_map(|stmt| stmt.0.iter())
            .any(|interp| matches!(interp, Interp::AuthParam(_param)));

        if has_auth && auth_settings.is_none() {
            // set to verify token if there is an auth token used
            auth_settings = Some(AuthSettings::VerifyToken(None))
        }

        let module = Self {
            auth: auth_settings,
            endpoint,
            sql,
            params: params.into_iter().map(String::from).collect(),
        };
        Ok((input, module))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(format!("{:?}", &module), "Module { auth: None, endpoint: None, params: [\"email\", \"id\"], sql: [Statement([Literal(\"select * from users \\nwhere id = \"), Param(\"id\"), Literal(\" \\nAND \"), Param(\"email\"), Literal(\" = \\\'testing 123 @haha\\\' \\nOR 0 = \"), Param(\"id\")])] }");

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
            "Failure(ErrorKind(\" \\nAND @email = \\\'testing 123 @haha\\\' \\nOR 0 = @id\", UndefinedParameterError(\"id\")))"
        );

        let test_str = r#"
/* @param email 
 * @param id
 */
select * from users 
where id = @id 
AND test(@email) = 'testing 123' 
OR 0 = @id;
        "#;
        let (input, sql) = Module::parse(test_str).unwrap();
        assert_eq!(input, "");
        assert!(sql
            .sql
            .iter()
            .flat_map(|stmt| stmt.0.iter())
            .all(|interp| match interp {
                Interp::Literal(lit) => lit.find('@').is_none(),
                _ => true,
            }))
    }
}
