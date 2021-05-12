use super::reserved_words::check_reserved_words;
use crate::codegen::{
    ast::Decorator,
    result::{CResult, IrErrorKind, ParseError},
    span_ref::SpanRef,
    AuthSettings, Module,
};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    ops::Deref,
    path::{Path, PathBuf},
};

// TODO: does this need to maintain span refs?
#[derive(Debug, Clone)]
pub struct FrontMatter {
    /// canonicalized location
    pub location: PathBuf,
    pub endpoint: Option<String>,
    pub params: Vec<String>,
    /// maps import name to canonicalized location and names of that module's parameters
    /// FIXME remove module parameter names this part would break if the importing module's
    /// changes it's parameter names during watch mode.
    pub imports: BTreeMap<String, (PathBuf, Vec<String>)>,
    pub auth_settings: Option<AuthSettings>,
}

impl FrontMatter {
    pub fn check_reserved_words<'a, 'b>(
        decorators: &'b Vec<SpanRef<'a, Decorator<'a>>>,
    ) -> impl Iterator<Item = ParseError<'a>> + 'b {
        let iter = decorators
            .iter()
            .filter_map(|decorator| match decorator.deref() {
                Decorator::Import(input, _path) => Some(*input),
                Decorator::Endpoint(keyword) | Decorator::Param(keyword) => {
                    Some(decorator.with(keyword))
                }
                Decorator::Auth(_) => None,
            });

        check_reserved_words(iter)
    }

    pub fn new<'a, P: Borrow<Path> + Ord>(
        location: PathBuf,
        mut decorators: Vec<SpanRef<'a, Decorator<'a>>>,
        modules: &BTreeMap<P, Module>,
    ) -> CResult<'a, Self> {
        // checking logic:
        //     1. all imports must not have conflicting names
        //     2. no parameter or import keyword can contain a reserved string
        //     3. all parameters cannot be duplicates
        //     4. all parameters cannot conflict with imports
        //     5. all imports are from modules with one statement
        //     6. all imports have modules
        //
        // for future consideration:
        //  if these rules get too complicated to maintain simply
        //  we should move to formalizing the checking semantics via a
        //  formal deductive logic system before adding more.

        decorators.sort_by_key(|k| match &*(k.as_ref()) {
            Decorator::Import(_, _) => 0,
            Decorator::Auth(_) => 1,
            Decorator::Endpoint(_) => 2,
            Decorator::Param(_) => 3,
        });

        let mut endpoint = None;
        let mut params: Vec<String> = vec![];
        let mut params_set = BTreeSet::new();
        let mut import_map = BTreeMap::new();
        let mut auth_settings = None;

        let mut deps = vec![];
        let mut errors = vec![];

        errors.extend(Self::check_reserved_words(&decorators));
        for decorator in decorators {
            match decorator.value {
                // imports
                Decorator::Import(name, file) => {
                    if import_map.get(name.value).is_some() {
                        errors.push(ParseError::const_error(
                            decorator.start,
                            "name already used for import",
                        ))
                    };

                    let mut location = location.clone();
                    location.push(file.value);

                    let location = match location.canonicalize() {
                        Ok(location) => location,
                        Err(_) => {
                            errors.push(ParseError::IrErrorKind(
                                file.start,
                                IrErrorKind::ConstError("could not import module"),
                            ));
                            continue;
                        }
                    };

                    let module = match modules.get(location.as_path()) {
                        Some(module) => {
                            if !module.is_single_statement() {
                                errors.push(ParseError::IrErrorKind(
                                    file.start,
                                    IrErrorKind::ConstError("Can not import sql file that is more than a single statement. Reduce this file to a single select, insert, delete or update statement."),
                                ));
                            }

                            deps.push(name.with(module));
                            module
                        }
                        None => continue,
                    };

                    let params = module.front_matter.params.iter().cloned().collect();

                    import_map.insert(name.to_string(), (location, params));
                }

                // auth settings
                Decorator::Auth(_) if auth_settings.is_some() => Result::Err(
                    ParseError::const_error(decorator.start, "multiple auth declarations detected"),
                )?,
                Decorator::Auth(val) => auth_settings = Some(val),

                // endpoints
                Decorator::Endpoint(dec) => match endpoint {
                    Some(_) => Result::Err(ParseError::const_error(
                        decorator.start,
                        "multiple endpoint declarations detected",
                    ))?,
                    None => {
                        endpoint = Some(dec.to_owned());
                    }
                },

                // parameters
                Decorator::Param(param) if params_set.contains(param) => Result::Err(
                    ParseError::const_error(decorator.start, "parameter already declared"),
                )?,
                Decorator::Param(param) if import_map.get(param).is_some() => Result::Err(
                    ParseError::const_error(decorator.start, "parameter is used for an import"),
                )?,
                Decorator::Param(param) => {
                    params.push(param.to_string());
                    params_set.insert(param);
                }
            };
        }

        if auth_settings.is_none() {
            errors.extend(deps.iter().filter_map(|dep| {
                if dep.front_matter.auth_settings.is_some() {
                    Some(ParseError::const_error(
                        dep.start,
                        "import requires auth but this module is does not check for auth. Add an '@auth validate' decorator.",
                    ))
                } else {
                    None
                }
            }))
        }

        if errors.len() == 0 {
            Ok(Self {
                location,
                endpoint: endpoint.map(|v| v.to_string()),
                params,
                imports: import_map,
                auth_settings,
            })
        } else if errors.len() == 1 {
            Err(errors.pop().unwrap())
        } else {
            Err(ParseError::Multiple(errors))
        }
    }
}
