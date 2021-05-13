use super::{
    ast::Ast,
    ir::{FrontMatter, Interp, Statements},
    result::{CResult, ParseError},
};
use crate::{
    binding::Binding,
    codegen::toposort::topological_sort,
    config::Secret,
    util::{
        error_printing::{print_error, print_unpositioned_error, PrintableError},
        mixed_ref::MixedRef,
        path::path_relative_to_current_dir,
    },
};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::{Path, PathBuf},
    pin::Pin,
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
    #[error("error in {0}: {1}")]
    SingleModuleError(PathBuf, SingleModuleError),
    #[error("there is a cyclic dependency")]
    CyclicDependency(Vec<PathBuf>),
}

#[derive(Error, Debug)]
pub enum SingleModuleError {
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

    pub fn with_parse_error<'a>(path: PathBuf, file_content: &'a str, err: ParseError<'a>) -> Self {
        if let Some((pos, error)) = Self::convert_simple_parse_error(file_content.borrow(), &err) {
            ModuleError::SingleModuleError(
                path,
                SingleModuleError::ParseError {
                    file: file_content.to_string(),
                    pos,
                    error,
                },
            )
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

            ModuleError::SingleModuleError(
                path,
                SingleModuleError::MultipleParseError {
                    file: file_content.to_string(),
                    errors: res,
                },
            )
        }
    }

    pub fn with_nom_error<'a>(
        path: PathBuf,
        file_content: &'a str,
        err: nom::Err<ParseError<'a>>,
    ) -> Self {
        return match err {
            nom::Err::Incomplete(_) => {
                ModuleError::SingleModuleError(path, SingleModuleError::Incomplete)
            }
            nom::Err::Failure(err) | nom::Err::Error(err) => {
                Self::with_parse_error(path, file_content, err)
            }
        };
    }
}

impl PrintableError for ModuleError {
    fn print_error<W: std::fmt::Write>(
        &self,
        writer: &mut W,
    ) -> Result<(), crate::util::error_printing::PrintError> {
        // FIXME change relative pathing to current dir

        match self {
            ModuleError::CyclicDependency(paths) => {
                for path in paths
                    .iter()
                    .map(PathBuf::as_path)
                    .map(path_relative_to_current_dir)
                {
                    let lossy = path.to_string_lossy();
                    let file_name = lossy.as_ref();
                    print_unpositioned_error(writer, "part of a dependency cycle", file_name)?
                }
            }
            ModuleError::SingleModuleError(path, err) => {
                let path = path_relative_to_current_dir(path.as_path());
                let lossy = path.to_string_lossy();
                let file_name = lossy.as_ref();
                match err {
                    SingleModuleError::IOError(_) | SingleModuleError::Incomplete => {
                        print_unpositioned_error(writer, err.to_string().as_ref(), file_name)?
                    }
                    SingleModuleError::MultipleParseError { file, errors } => {
                        for (pos, err) in errors.iter() {
                            print_error(writer, file.as_str(), *pos, err.as_str(), file_name)?;
                            write!(writer, "\n")?;
                        }
                    }
                    SingleModuleError::ParseError { file, pos, error } => {
                        print_error(writer, file.as_str(), *pos, error.as_str(), file_name)?
                    }
                }
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

    pub fn new<'a, P: Borrow<Path> + Ord, M: Borrow<Module>>(
        ast: Ast<'a>,
        modules: &BTreeMap<P, M>,
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
        Self::new(ast, &BTreeMap::<&Path, &Module>::new()).map_err(nom::Err::Failure)
    }

    /// FIXME this only works for single modules with no import statements
    pub fn from_path<'a>(path: &'a Path) -> Result<Module, ModuleError> {
        use std::io::prelude::*;
        let mut file = std::fs::File::open(path)
            .map_err(SingleModuleError::IOError)
            .map_err(|single_module_error| {
                ModuleError::SingleModuleError(path.to_path_buf(), single_module_error)
            })?;
        let mut file_content = String::new();
        file.read_to_string(&mut file_content)
            .map_err(SingleModuleError::IOError)
            .map_err(|single_module_error| {
                ModuleError::SingleModuleError(path.to_path_buf(), single_module_error)
            })?;
        // TODO file content needs to be copied twice
        // figure out a way to handle this without a copy.
        Self::from_str(path.to_path_buf(), file_content.as_str()).map_err(|err| {
            ModuleError::with_nom_error(path.to_path_buf(), file_content.as_str().into(), err)
        })
    }

    fn gen_file_contents<'a>(
        errors: &mut Vec<ModuleError>,
        paths: &'a [PathBuf],
    ) -> BTreeMap<&'a Path, String> {
        use std::io::prelude::*;
        let mut file_contents = BTreeMap::new();
        for path in paths.iter() {
            let mut file = match std::fs::File::open(path) {
                Ok(file) => file,
                Err(err) => {
                    errors.push(ModuleError::SingleModuleError(
                        path.to_path_buf(),
                        SingleModuleError::IOError(err),
                    ));
                    continue;
                }
            };
            let mut file_content = String::new();

            if let Err(err) = file.read_to_string(&mut file_content) {
                errors.push(ModuleError::SingleModuleError(
                    path.to_path_buf(),
                    SingleModuleError::IOError(err),
                ));
                continue;
            }

            file_contents.insert(path.as_path(), file_content);
        }

        file_contents
    }

    pub fn gen_asts<'b>(
        errors: &mut Vec<ModuleError>,
        deps: &BTreeSet<PathBuf>,
        paths: &[PathBuf],
        file_contents: &'b BTreeMap<PathBuf, Pin<Box<String>>>,
    ) -> (BTreeMap<PathBuf, Ast<'b>>, BTreeSet<PathBuf>, Vec<PathBuf>) {
        let mut failed_imports = BTreeSet::new();
        let asts: BTreeMap<PathBuf, Ast<'b>> = paths
            .iter()
            .cloned()
            .filter_map(|path| {
                // filter out the things that failed in the previous pass
                let contents: &'b str = file_contents.get(path.as_path())?.as_str();
                let ast_res = Ast::parse(path.to_path_buf(), contents).map(|v| v.1);
                match ast_res {
                    Ok(v) => Some((path, v)),
                    Err(err) => {
                        failed_imports.insert(path.clone());
                        errors.push(ModuleError::with_nom_error(path, contents, err));
                        None
                    }
                }
            })
            .collect();

        let unimported_paths = asts
            .values()
            .flat_map(|ast| ast.canonicalized_dependencies())
            .map(|span_ref| span_ref.value)
            .filter(|path| {
                let path = path.as_path();
                !(asts.contains_key(path) || deps.contains(path) || failed_imports.contains(path))
            })
            .collect();

        (asts, failed_imports, unimported_paths)
    }

    // paths should all be canonical paths
    // note this can return more paths than you put in
    pub fn from_paths<'a, M: Borrow<Module>>(
        paths: &[&'a Path],
        deps: Option<&BTreeMap<&'a Path, M>>,
    ) -> (BTreeMap<PathBuf, Module>, Vec<ModuleError>) {
        let mut errors = vec![];

        let mut file_contents = BTreeMap::new();

        let mut asts: BTreeMap<PathBuf, Ast> = BTreeMap::new();
        let mut path_deps = deps
            .iter()
            .flat_map(|map| map.keys())
            .cloned()
            .map(Path::to_path_buf)
            .collect();
        let mut to_import: Vec<PathBuf> = paths.iter().cloned().map(Path::to_path_buf).collect();

        // loop over asts to parse in case the ast list doesn't include all the things the module
        // needs to import
        while to_import.len() != 0 {
            file_contents.extend(
                Self::gen_file_contents(&mut errors, to_import.as_slice())
                    .into_iter()
                    .map(|(path, content)| (path.to_path_buf(), Box::pin(content))),
            );
            let (new_asts, failed_imports, next_imports) = Self::gen_asts(
                &mut errors,
                &path_deps,
                to_import.as_slice(),
                &file_contents,
            );
            path_deps.extend(new_asts.keys().cloned());
            path_deps.extend(failed_imports);
            // NOTE must ensure asts can only maintain references to the content of the string
            asts.extend(new_asts.into_iter().map(|(path, ast)| {
                (path, unsafe {
                    // this is safe because asts only maintain references to the pinned string
                    std::mem::transmute::<Ast<'_>, Ast<'static>>(ast)
                })
            }));
            to_import = next_imports;
        }

        // finally topologically sort by ast and complete the rest in topological order
        // currently asts maintain the order that paths came in from the argument
        let edges: Vec<(PathBuf, PathBuf)> = asts
            .iter()
            .flat_map(|(path, ast)| {
                ast.canonicalized_dependencies()
                    .filter_map(move |path_buf| Some((path.to_path_buf(), path_buf.value)))
            })
            .collect();

        let (sorted, sorting_errors) = topological_sort(edges.iter());
        if let Some(set) = sorting_errors {
            errors.push(ModuleError::CyclicDependency(
                set.into_iter().map(|v| v.to_path_buf()).collect(),
            ));
        };

        let mut modules: BTreeMap<PathBuf, MixedRef<Module>> = BTreeMap::new();
        modules.extend(deps.iter().flat_map(|map| {
            map.iter()
                .map(|(key, value)| (key.to_path_buf(), MixedRef::Borrowed(value.borrow())))
        }));

        let mut errors: Vec<ModuleError> = vec![];

        for (path, contents, ast) in sorted
            .into_iter()
            .map(PathBuf::as_path)
            .filter_map(|path| Some((path, file_contents.get(path)?.as_str(), asts.remove(path)?)))
        {
            match Module::new(ast, &modules)
                .map_err(|err| ModuleError::with_parse_error(path.to_path_buf(), contents, err))
            {
                Ok(res) => {
                    modules.insert(path.to_path_buf(), MixedRef::Owned(res));
                }
                Err(err) => errors.push(err),
            }
        }

        let new_modules = modules
            .into_iter()
            .filter_map(|(path, module)| match module {
                // filters out the existing dependencies that were mixed in
                MixedRef::Borrowed(_) => None,
                MixedRef::Owned(v) => Some((path, v)),
            })
            .collect();

        drop(file_contents);
        (new_modules, errors)
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

                // FIXME add callsites
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
