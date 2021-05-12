use super::{
    decorator::{parse_decorators, Decorator},
    sql::{parse_statements, StatementSpan},
};
use crate::codegen::{
    result::{ErrorKind, PResult, ParseError},
    span_ref::SpanRef,
};
use nom::combinator::eof;
use std::path::PathBuf;

/// the abstract syntax tree (AST)
#[derive(Debug, Clone)]
pub struct Ast<'a> {
    pub file_loc: PathBuf,
    pub decorators: Vec<SpanRef<'a, Decorator<'a>>>,
    pub statements: Vec<SpanRef<'a, StatementSpan<'a>>>,
}

impl<'a> Ast<'a> {
    pub fn parse(file_loc: PathBuf, input: &'a str) -> PResult<'a, Self> {
        let (input, decorators) = parse_decorators(input)?;
        let (input, statements) = parse_statements(input)?;
        let (input, _) = eof(input).map_err(|_: nom::Err<ParseError>| {
            nom::Err::Failure(ParseError::error_kind(
                input,
                ErrorKind::ConstError("expected end of file"),
            ))
        })?;
        Ok((
            input,
            Self {
                file_loc,
                decorators,
                statements,
            },
        ))
    }

    pub fn canonicalized_dependencies(&self) -> impl Iterator<Item = SpanRef<'a, PathBuf>> + '_ {
        self.dependencies()
            .filter_map(|dep| dep.with(dep.canonicalize()).transpose().ok())
    }

    pub fn dependencies(&self) -> impl Iterator<Item = SpanRef<'a, PathBuf>> + '_ {
        let file_loc = self.file_loc.as_path();
        self.decorators
            .iter()
            .filter_map(move |decorator| match &decorator.value {
                Decorator::Import(_, path) => path
                    .map(|path| {
                        let mut cur_loc = file_loc.to_path_buf();
                        cur_loc.push(path);
                        Some(cur_loc)
                    })
                    .transpose(),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::codegen::ast::InterpSpan;

    use super::*;

    fn assert_valid_ast(
        test_str: &str,
        expected_decorators: Vec<&Decorator>,
        expected_params: Vec<&InterpSpan>,
        expected_statements: usize,
    ) {
        let path = PathBuf::new();
        let (_, ast) = Ast::parse(path.clone(), test_str).unwrap();
        let decorators: Vec<_> = ast.decorators.iter().map(|span| &span.value).collect();
        assert_eq!(decorators, expected_decorators,);
        let params: Vec<_> = ast
            .statements
            .iter()
            .flat_map(|span| span.value.0.iter())
            .filter(|interp_span| match &interp_span.value {
                InterpSpan::Param(_) | InterpSpan::AuthParam(_) => true,
                _ => false,
            })
            .map(|span| &span.value)
            .collect();
        assert_eq!(params, expected_params);
        assert_eq!(ast.statements.len(), expected_statements);
    }

    #[test]
    fn valid_ast_tests() {
        let test_str = r#"
-- @param email
-- @param id 
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id"#;
        assert_valid_ast(
            test_str,
            vec![&Decorator::Param("email"), &Decorator::Param("id")],
            vec![
                &InterpSpan::Param("id"),
                &InterpSpan::Param("email"),
                &InterpSpan::Param("id"),
            ],
            1,
        );

        let test_str = r#"
-- @param email
-- @param id 
select * from users"#;
        assert_valid_ast(
            test_str,
            vec![&Decorator::Param("email"), &Decorator::Param("id")],
            vec![],
            1,
        );

        let test_str = r#"
-- @import test from './hello_world.txt'
-- @import test2 from './hello_world2.txt'
select * from test"#;
        let deps: Vec<_> = Ast::parse(PathBuf::new(), test_str)
            .unwrap()
            .1
            .dependencies()
            .map(|spans| spans.value)
            .collect();

        assert_eq!(
            deps,
            vec![
                Path::new("./hello_world.txt"),
                Path::new("./hello_world2.txt")
            ]
        );
    }

    #[test]
    fn invalid_ast_test() {
        let path = PathBuf::new();
        let test_str = r#"
-- @param email
-- @param id 
; ; ;"#;
        assert_eq!(
            Ast::parse(path.clone(), test_str).unwrap_err().to_string(),
            "Parsing Error: ErrorKind(\"; ; ;\", ConstError(\"statement(s) are empty\"))"
        );
    }
}
