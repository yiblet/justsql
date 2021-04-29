use crate::{
    ast::{
        decorator::Decorator,
        parser::{const_error, normalize_sql, PResult},
    },
    server::auth::decode,
};
use anyhow::anyhow;
use nom::{combinator::eof, multi::many_till, Err};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone, PartialEq)]
pub enum AuthSettings {
    VerifyToken(Option<u64>),
    SetToken(u64), // number of seconds till expiration
    RemoveToken,
}

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone, Default)]
pub struct Module {
    pub auth: Option<AuthSettings>,
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

    pub fn verify(&self, cookie: Option<&str>) -> anyhow::Result<()> {
        if matches!(self.auth, Some(AuthSettings::VerifyToken(_))) {
            return decode(cookie.ok_or_else(|| anyhow!("missing cookie"))?).map(|_| ());
        }
        Ok(())
    }

    pub fn bindings<'a, 'b: 'a, Q, T>(
        &'b self,
        bindings: &'a BTreeMap<Q, T>,
    ) -> impl Iterator<Item = anyhow::Result<&'a T>>
    where
        Q: Borrow<str> + Ord,
    {
        self.params.iter().map(move |param| {
            bindings
                .get(param.as_str())
                .ok_or_else(|| anyhow!("parameter {} is not bound to an argument", param))
        })
    }

    pub fn from_path<A: AsRef<Path>>(input: A) -> anyhow::Result<Module> {
        use std::io::prelude::*;
        let path = input.as_ref();
        let mut file = std::fs::File::open(path)?;
        let mut file_content = String::with_capacity(file.metadata()?.len() as usize);
        file.read_to_string(&mut file_content)?;
        let (_, data) =
            Module::parse(file_content.as_str()).map_err(|err| anyhow!("{}", err.to_string()))?;
        Ok(data)
    }

    pub fn parse(input: &str) -> PResult<Self> {
        let (input, decorators) = crate::ast::decorator::frontmatter(input)?;

        let mut endpoint = None;
        let mut params = vec![];
        let mut params_set = BTreeSet::new();
        let mut auth = None;

        for decorator in decorators.into_iter() {
            match decorator {
                Decorator::Auth(_) if auth.is_some() => Result::Err(Err::Failure(const_error(
                    input,
                    "multiple auth declarations detected",
                )))?,
                Decorator::Auth(val) => auth = Some(val),
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
            auth,
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
    fn module_parse_test() {
        let test_str = r#"
-- @param email
-- @param id 
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id"#;
        let (_, module) = Module::parse(test_str).unwrap();
        assert_eq!(format!("{:?}", &module), "Module { auth: None, endpoint: None, params: [\"email\", \"id\"], sql: [\"select * from users \\nwhere id = $1 \\nAND $2 = \\\'testing 123 @haha\\\' \\nOR 0 = $1\"] }");

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
            vec!["$1".to_owned(), "test".to_owned(), "test".to_owned(),]
        );
    }
}
