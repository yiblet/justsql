use super::{
    ast::{Ast, Decorators},
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParamType {
    Auth(String),
    Param(String),
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

    pub fn from_str<'a>(path: PathBuf, data: &'a str) -> CResult<'a, Self> {
        let (_, ast) = Ast::parse(path, data).map_err(|err| match err {
            nom::Err::Incomplete(_) => ParseError::const_error(data, "incomplete"),
            nom::Err::Error(err) => err,
            nom::Err::Failure(err) => err,
        })?;
        Ok(Self::new::<&Path, Module>(ast, &BTreeMap::new())?)
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

        let front_matter = FrontMatter::new(file_loc, decorators.into_inner(), modules)?;
        let statements = Statements::new(&front_matter, statements)?;
        Ok(Self {
            front_matter,
            sql: statements.0,
        })
    }

    fn read_file<'a>(path: &'a Path) -> Result<String, ModuleError> {
        use std::io::prelude::*;
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(err) => Err(ModuleError::SingleModuleError(
                path.to_path_buf(),
                SingleModuleError::IOError(err),
            ))?,
        };
        let mut file_content = String::new();
        if let Err(err) = file.read_to_string(&mut file_content) {
            Err(ModuleError::SingleModuleError(
                path.to_path_buf(),
                SingleModuleError::IOError(err),
            ))?;
        }

        Ok(file_content)
    }

    fn gen_file_contents<'a, M>(
        errors: &mut Vec<ModuleError>,
        paths: &[&'a Path],
        deps: Option<&BTreeMap<&'a Path, M>>,
    ) -> BTreeMap<PathBuf, String> {
        let mut file_contents = BTreeMap::new();
        let mut imports = BTreeSet::new();
        let mut paths = paths
            .into_iter()
            .cloned()
            .map(Path::to_path_buf)
            .collect::<Vec<_>>();

        while let Some(path) = paths.pop() {
            if imports.contains(path.as_path()) {
                continue;
            }
            imports.insert(path.clone());

            match Self::read_file(path.as_path()) {
                Ok(file_content) => {
                    if let Some((_, decorators)) = Decorators::parse(file_content.as_str()).ok() {
                        let new_deps = decorators
                            .canonicalized_dependencies(path.as_path())
                            .map(|span_ref| span_ref.value)
                            .filter(|path| {
                                // filter out dependencies that shouldn't be imported
                                deps.map_or(true, |deps| !deps.contains_key(path.as_path()))
                            });
                        paths.extend(new_deps);
                    }
                    file_contents.insert(path, file_content);
                }
                Err(err) => {
                    errors.push(err);
                }
            };
        }

        file_contents
    }

    pub fn gen_asts<'b>(
        errors: &mut Vec<ModuleError>,
        file_contents: &'b BTreeMap<PathBuf, String>,
    ) -> BTreeMap<PathBuf, Ast<'b>> {
        let asts: BTreeMap<PathBuf, Ast<'b>> = file_contents
            .iter()
            .filter_map(|(path, contents)| {
                // filter out the things that failed in the previous pass
                let ast_res = Ast::parse(path.clone(), contents).map(|v| v.1);
                match ast_res {
                    Ok(v) => Some((path.clone(), v)),
                    Err(err) => {
                        errors.push(ModuleError::with_nom_error(path.clone(), contents, err));
                        None
                    }
                }
            })
            .collect();

        asts
    }

    // paths should all be canonical paths
    // TODO create an interner for paths and file_contents so that we can use references through out the build process.
    // TODO split module parsing to it's own builder pattern-style struct
    // note this can return more paths than you put in
    pub fn from_paths<'a, M: Borrow<Module>>(
        paths: &[&'a Path],
        deps: Option<&BTreeMap<&'a Path, M>>,
    ) -> (BTreeMap<PathBuf, Module>, Vec<ModuleError>) {
        let mut modules: BTreeMap<PathBuf, MixedRef<Module>> = BTreeMap::new();
        let mut errors = vec![];

        debug!("number of paths: {}", paths.len());
        // all imported file contents are exactly the file_contents that are in paths or their
        // (dependencies - deps) excluding files that we failed to import.
        let file_contents = Self::gen_file_contents(&mut errors, paths, deps.clone());
        debug!("number of files read: {}", file_contents.len());
        // asts contain exactly all asts that should be imported excluding those that errored out
        let mut asts = Self::gen_asts(&mut errors, &file_contents);
        debug!("number of ASTs parsed: {}", asts.len());

        // finally topologically sort by ast and complete the rest in topological order
        // currently asts maintain the order that paths came in from the argument
        let mut nodes: BTreeSet<PathBuf> = asts.keys().cloned().collect();
        let mut edges: Vec<(PathBuf, PathBuf)> = vec![];
        for (path, ast) in asts.iter() {
            for dep in ast.canonicalized_dependencies() {
                if !nodes.contains(&dep.value) {
                    nodes.insert(dep.value.clone());
                }
                edges.push((path.clone(), dep.value))
            }
        }

        let (sorted, sorting_errors) = topological_sort(nodes.iter(), edges.iter());
        if let Some(set) = sorting_errors {
            errors.push(ModuleError::CyclicDependency(
                set.into_iter().map(|v| v.to_path_buf()).collect(),
            ));
        };

        modules.extend(deps.iter().flat_map(|map| {
            map.iter()
                .map(|(key, value)| (key.to_path_buf(), MixedRef::Borrowed(value.borrow())))
        }));

        for (path, contents, ast) in sorted
            .into_iter()
            .map(PathBuf::as_path)
            // filters out paths that are dependencies but do not need to be imported
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
            .collect::<BTreeMap<_, _>>();

        drop(file_contents);

        debug!(
            "imported {} new modules, with {} errors",
            new_modules.len(),
            errors.len()
        );
        (new_modules, errors)
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
            "Multiple([ErrorKind(\"@id \\nAND @email = \\\'testing 123 @haha\\\' \\nOR 0 = @id\", UndefinedParameterError(\"id\")), ErrorKind(\"@id\", UndefinedParameterError(\"id\"))])"
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
