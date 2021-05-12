use super::{
    ast::Ast,
    ir::{FrontMatter, Interp, Statements},
    result::{CResult, ParseError},
};
use crate::{
    binding::Binding,
    codegen::toposort::topological_sort,
    config::Secret,
    engine::ModuleCollectionError,
    util::{
        error_printing::{print_error, print_unpositioned_error, PrintableError},
        path::path_relative_to_current_dir,
    },
};
use futures::future::err;
use std::{
    borrow::{Borrow, Cow},
    collections::BTreeMap,
    fmt::Write,
    path::{Path, PathBuf},
};
use thiserror::Error;

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone, PartialEq)]
pub enum AuthSettings {
    VerifyToken(Option<u64>),
    SetToken(u64), // number of seconds till expiration
    RemoveToken,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Copy)]
pub enum ParamType<'a> {
    Auth(&'a str),
    Param(&'a str),
}

#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("multiple errors")]
    MultipleParseError {
        file: String,
        errors: Vec<(usize, String)>,
    },
    #[error("{error}")]
    ParseError {
        file: String,
        pos: usize,
        error: String,
    },
    #[error("file is incomplete")]
    Incomplete,
    #[error("there is a cyclic dependency")]
    CyclicDependency(Vec<PathBuf>),
    #[error("multiple errors")]
    Multiple(Vec<ModuleError>),
}

impl ModuleError {
    fn convert_simple_parse_error<'a>(
        file_content: &'a str,
        err: &ParseError<'a>,
    ) -> Option<(usize, String)> {
        return match err {
            ParseError::NomError(input, _) => {
                let pos = file_content.len() - input.len();
                Some((pos, "unexpected token".to_string()))
            }
            ParseError::IrErrorKind(input, kind) => {
                let pos = file_content.len() - input.len();
                let error = format!("{}", kind);
                Some((pos, error))
            }
            ParseError::ErrorKind(input, kind) => {
                let pos = file_content.len() - input.len();
                let error = format!("{}", kind);
                Some((pos, error))
            }
            ParseError::Multiple(_) => None,
        };
    }

    pub fn with_parse_error<'a>(file_content: Cow<'a, str>, err: ParseError<'a>) -> Self {
        if let Some((pos, error)) = Self::convert_simple_parse_error(file_content.borrow(), &err) {
            ModuleError::ParseError {
                file: file_content.into_owned(),
                pos,
                error,
            }
        } else {
            let mut errors = match err {
                ParseError::Multiple(errors) => errors,
                _ => {
                    panic!("all non multiple parse errors should be simple")
                }
            };
            let mut res = Vec::with_capacity(errors.len());

            while let Some(err) = errors.pop() {
                if let Some(val) = Self::convert_simple_parse_error(file_content.borrow(), &err) {
                    res.push(val)
                } else {
                    match err {
                        ParseError::Multiple(new_errors) => {
                            errors.extend(new_errors);
                        }
                        _ => {
                            panic!("all non multiple parse errors should be simple")
                        }
                    }
                }
            }

            // sort the errors by position so that errors are ordered by line
            res.sort_by_key(|(pos, _)| *pos);

            ModuleError::MultipleParseError {
                file: file_content.into_owned(),
                errors: res,
            }
        }
    }

    pub fn with_nom_error<'a>(file_content: Cow<'a, str>, err: nom::Err<ParseError<'a>>) -> Self {
        return match err {
            nom::Err::Incomplete(_) => ModuleError::Incomplete,
            nom::Err::Failure(err) | nom::Err::Error(err) => {
                Self::with_parse_error(file_content, err)
            }
        };
    }
}

impl PrintableError for ModuleError {
    fn print_error<W: std::fmt::Write>(
        &self,
        writer: &mut W,
        file_name: &str,
    ) -> Result<(), crate::util::error_printing::PrintError> {
        // FIXME change relative pathing to current dir
        let path = path_relative_to_current_dir(Path::new(file_name).to_path_buf());
        let lossy = path.to_string_lossy();
        let file_name = lossy.as_ref(); // FIXME module errors must now contain the module they pointed to

        match self {
            ModuleError::Multiple(errors) => {
                for error in errors.iter() {
                    error.print_error(writer, file_name)?;
                }
            }
            ModuleError::CyclicDependency(paths) => {
                eprintln!("cyclic dependency in at least one of the following files:");
                for path in paths.iter().cloned().map(path_relative_to_current_dir) {
                    eprintln!("\t {}", path.to_string_lossy());
                }
            }
            ModuleError::IOError(_) | ModuleError::Incomplete => {
                print_unpositioned_error(writer, self.to_string().as_ref(), file_name)?
            }
            ModuleError::MultipleParseError { file, errors } => {
                for (pos, err) in errors.iter() {
                    print_error(writer, file.as_str(), *pos, err.as_str(), file_name)?
                }
            }
            ModuleError::ParseError { file, pos, error } => {
                print_error(writer, file.as_str(), *pos, error.as_str(), file_name)?
            }
        };

        Ok(())
    }
}

// TODO set up "pre-interpolated" sql type
#[derive(Debug, Clone)]
pub struct Module {
    pub front_matter: FrontMatter,
    pub sql: Vec<Vec<Interp>>,
}

impl Module {
    pub fn verify(
        &self,
        secret: Option<&Secret>,
        cookie: Option<&str>,
    ) -> anyhow::Result<Option<BTreeMap<String, Binding>>> {
        if matches!(
            &self.front_matter.auth_settings,
            Some(AuthSettings::VerifyToken(_))
        ) {
            return secret
                .ok_or_else(|| anyhow!("secret is needed to verify cookie auth"))?
                .decode(cookie.ok_or_else(|| anyhow!("missing cookie"))?)
                .map(|claim| Some(claim.claims));
        }
        Ok(None)
    }

    /// only modules that are single statements can be imported and reused inside
    /// of common table expression. We expose a utility function that identifies this.
    pub fn is_single_statement(&self) -> bool {
        self.sql.len() == 1
    }

    pub fn new<'a, P: Borrow<Path> + Ord>(
        ast: Ast<'a>,
        modules: &BTreeMap<P, Module>,
    ) -> CResult<'a, Self> {
        let Ast {
            file_loc,
            decorators,
            statements,
        } = ast;

        let front_matter = FrontMatter::new(file_loc, decorators, modules)?;
        let statements = Statements::new(&front_matter, statements)?;
        Ok(Self {
            front_matter,
            sql: statements.0,
        })
    }

    pub fn from_str(file_loc: PathBuf, input: &str) -> Result<Module, nom::Err<ParseError>> {
        let (_, ast) = Ast::parse(file_loc, input)?;
        Self::new(ast, &BTreeMap::<&Path, _>::new()).map_err(nom::Err::Failure)
    }

    /// FIXME this only works for single modules with no import statements
    pub fn from_path<'a>(path: &'a Path) -> Result<Module, ModuleError> {
        use std::io::prelude::*;
        let mut file = std::fs::File::open(path)?;
        let mut file_content = String::with_capacity(file.metadata()?.len() as usize);
        file.read_to_string(&mut file_content)?;
        // TODO file content needs to be copied twice
        // figure out a way to handle this without a copy.
        Self::from_str(path.to_path_buf(), file_content.as_str())
            .map_err(|err| ModuleError::with_nom_error(file_content.as_str().into(), err))
    }

    // paths should all be canonical paths
    // TODO make existing modules work for watching importer
    pub fn from_paths<'a>(paths: &[&'a Path]) -> Result<BTreeMap<&'a Path, Module>, ModuleError> {
        use std::io::prelude::*;
        let mut file_contents = vec![];

        // first parse paths into path contents
        let mut path_mapping: BTreeMap<&Path, usize> = BTreeMap::new();
        let paths_iter = paths.iter().cloned();

        for (idx, path) in paths_iter.clone().enumerate() {
            path_mapping.insert(path, idx);
            let mut file = std::fs::File::open(path)?;
            let mut file_content = String::new();
            file.read_to_string(&mut file_content)?;
            file_contents.push(file_content);
        }

        let contents = file_contents.as_slice();

        // parse asts
        let asts: Vec<Result<Ast, ModuleError>> = paths_iter
            .enumerate()
            .map(|(idx, path)| {
                Ast::parse(path.to_path_buf(), contents[idx].as_str())
                    .map(|v| v.1)
                    .map_err(|err| {
                        ModuleError::with_nom_error(Cow::Borrowed(contents[idx].as_str()), err)
                    })
            })
            .collect();

        if asts.iter().any(Result::is_err) {
            Err(ModuleError::Multiple(
                asts.into_iter()
                    .filter(Result::is_err)
                    .map(Result::unwrap_err)
                    .collect(),
            ))?
        } else {
            // finally topologically sort by ast and complete the rest in topological order

            // all asts are guarenteed to have no errors because
            // this line cannot be evaluated if an ast has an error
            let mut asts: Vec<Option<Ast>> = asts.into_iter().map(Result::ok).collect();
            // currently asts maintain the order that paths came in from the argument
            let edges: Vec<(usize, usize)> = asts
                .iter()
                .enumerate()
                .flat_map(|(idx, ast)| {
                    let path_mapping_ref = &path_mapping;
                    ast.iter().flat_map(move |ast| {
                        ast.canonicalized_dependencies()
                            .filter_map(move |path_buf| {
                                let dep_idx = path_mapping_ref.get(path_buf.value.as_path())?;
                                Some((idx, *dep_idx))
                            })
                    })
                })
                .collect();

            let sorted = topological_sort(edges.iter()).map_err(|set| {
                ModuleError::CyclicDependency(
                    set.into_iter()
                        .filter_map(|v| paths.get(*v).cloned().map(Path::to_path_buf))
                        .collect(),
                )
            })?;
            assert_eq!(sorted.len(), asts.len());

            let mut modules = BTreeMap::new();
            let mut errors: Vec<ModuleError> = vec![];

            for (idx, ast) in sorted
                .into_iter()
                .filter_map(|idx| Some((*idx, asts.get_mut(*idx)?.take()?)))
            {
                match Module::new(ast, &modules).map_err(|err| {
                    ModuleError::with_parse_error(Cow::Borrowed(&file_contents[idx]), err)
                }) {
                    Ok(res) => {
                        modules.insert(paths[idx], res);
                    }
                    Err(err) => errors.push(err),
                }
            }

            if errors.len() == 1 {
                Err(errors.pop().unwrap())
            } else if errors.len() > 1 {
                Err(ModuleError::Multiple(errors))
            } else {
                Ok(modules)
            }
        }
    }

    pub fn evaluate<'a, A>(
        &self,
        bindings: &'a BTreeMap<String, A>,
        auth_bindings: Option<&'a BTreeMap<String, A>>,
    ) -> anyhow::Result<Vec<(String, Vec<&'a A>)>> {
        self.sql
            .iter()
            .map(|stmt| -> anyhow::Result<(String, Vec<&'a A>)> {
                let (res, mapping) = Self::bind(stmt.iter())?;
                let bindings: Vec<_> = mapping
                    .into_iter()
                    .map(|param| match param {
                        ParamType::Param(param) => bindings
                            .get(param)
                            .ok_or_else(|| anyhow!("parameter {} does not exist", param)),
                        ParamType::Auth(param) => auth_bindings
                            .ok_or_else(|| anyhow!("must be authorized"))?
                            .get(param)
                            .ok_or_else(|| anyhow!("parameter {} does not exist", param)),
                    })
                    .collect::<anyhow::Result<_>>()?;
                Ok((res, bindings))
            })
            .collect()
    }

    pub fn bind<'a, I: Iterator<Item = &'a Interp>>(
        iter: I,
    ) -> anyhow::Result<(String, Vec<ParamType<'a>>)> {
        let mut params = vec![];
        let mut mapping: BTreeMap<ParamType, usize> = BTreeMap::new();
        let mut res = String::new();
        for interp in iter {
            match &interp {
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

                _ => todo!(),
            }
        }
        Ok((res, params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_parse_test() {
        let path = PathBuf::new();
        let test_str = r#"
-- @param email
-- @param id 
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id"#;
        let module = Module::from_str(path.clone(), test_str).unwrap();
        assert_eq!(format!("{:?}", &module), "Module { front_matter: FrontMatter { location: \"\", endpoint: None, params: [\"email\", \"id\"], imports: {}, auth_settings: None }, sql: [[Literal(\"select * from users \\nwhere id = \"), Param(\"id\"), Literal(\" \\nAND \"), Param(\"email\"), Literal(\" = \\\'testing 123 @haha\\\' \\nOR 0 = \"), Param(\"id\")]] }");

        let test_str = r#"
/* @param email 
 * 
 */
select * from users 
where id = @id 
AND @email = 'testing 123 @haha' 
OR 0 = @id"#;
        let err = Module::from_str(path.clone(), test_str).unwrap_err();
        assert_eq!(
            format!("{:?}", &err)
            ,
            "Failure(Multiple([ErrorKind(\"@id \\nAND @email = \\\'testing 123 @haha\\\' \\nOR 0 = @id\", UndefinedParameterError(\"id\")), ErrorKind(\"@id\", UndefinedParameterError(\"id\"))]))"
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
        let module = Module::from_str(path.clone(), test_str).unwrap();
        assert!(module
            .sql
            .iter()
            .flat_map(|stmt| stmt.iter())
            .all(|interp| match interp {
                Interp::Literal(lit) => lit.find('@').is_none(),
                _ => true,
            }))
    }
}
