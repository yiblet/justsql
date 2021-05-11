use super::{
    super::{
        ast::{InterpSpan, StatementSpan},
        result::IrErrorKind,
        result::{CResult, ErrorKind, ParseError},
        span_ref::SpanRef,
    },
    front_matter::FrontMatter,
    reserved_words::check_reserved_words,
};
use std::{collections::BTreeSet, iter};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Statements(pub Vec<Vec<Interp>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interp {
    Literal(String),
    Param(String),
    AuthParam(String),
    // TODO allow for expressions inside call sites
    CallSite(String, Vec<String>),
}

impl Interp {
    pub fn from<'a>(span: &InterpSpan<'a>) -> Self {
        match span {
            InterpSpan::Literal(lit) => Self::Literal(lit.to_string()),
            InterpSpan::Param(param) => Self::Param(param.to_string()),
            InterpSpan::AuthParam(param) => Self::AuthParam(param.to_string()),
            InterpSpan::CallSite(func, arg) => Self::CallSite(
                func.to_string(),
                arg.iter().map(|val| val.to_string()).collect(),
            ),
        }
    }
}

impl Statements {
    fn check_reserved_words<'a, 'b>(
        sql: &'b Vec<SpanRef<'a, StatementSpan<'a>>>,
    ) -> impl Iterator<Item = ParseError<'a>> + 'b {
        let iter = sql.iter().flat_map(|statement| {
            statement.0.iter().flat_map(|interp| {
                // need to use dynamic dispatch to allow for multiple return types
                let iter: Box<dyn Iterator<Item = SpanRef<'a, &str>>> = match &interp.value {
                    InterpSpan::Literal(lit) => {
                        Box::new(iter::once(interp.as_ref().map(|_| lit.as_str())))
                    }
                    InterpSpan::Param(param) | InterpSpan::AuthParam(param) => {
                        Box::new(iter::once(interp.as_ref().map(|_| *param)))
                    }
                    InterpSpan::CallSite(func, args) => Box::new(
                        iter::once(interp.as_ref().map(|_| *func)).chain(args.iter().cloned()),
                    ),
                };

                iter
            })
        });

        check_reserved_words(iter)
    }

    fn check_for_errors<'a>(
        front_matter: &FrontMatter,
        sql: &Vec<SpanRef<'a, StatementSpan<'a>>>,
    ) -> Vec<ParseError<'a>> {
        let params_set: BTreeSet<_> = front_matter.params.iter().map(String::as_str).collect();
        let mut errors = vec![];

        for interp_ref in sql.iter().flat_map(|stmt| stmt.value.0.iter()) {
            match &interp_ref.value {
                InterpSpan::CallSite(func, args) => {
                    // if function does not exist
                    match front_matter.imports.get(*func) {
                        None => errors.push(ParseError::IrErrorKind(
                            interp_ref.start,
                            IrErrorKind::UndefinedFunctionError(func.to_string()),
                        )),
                        Some((_, func_args)) if func_args.len() != args.len() => {
                            errors.push(ParseError::IrErrorKind(
                                interp_ref.start,
                                IrErrorKind::WrongNumberArgumentsError(func_args.len(), args.len()),
                            ))
                        }
                        Some(_) => {}
                    }

                    for arg in args.iter() {
                        if !params_set.contains(arg.value) {
                            errors.push(ParseError::error_kind(
                                interp_ref.start,
                                ErrorKind::UndefinedArgumentError(
                                    arg.to_string(),
                                    func.to_string(),
                                ),
                            ))
                        }
                    }
                }

                InterpSpan::Param(param) if !params_set.contains(param) => {
                    errors.push(ParseError::error_kind(
                        interp_ref.start,
                        ErrorKind::UndefinedParameterError(param.to_string()),
                    ))
                }
                _ => {}
            }
        }

        let has_auth = sql
            .iter()
            .flat_map(|stmt| stmt.0.iter())
            .find(|interp| matches!(interp.value, InterpSpan::AuthParam(_)));

        if let Some(auth) = has_auth {
            if front_matter.auth_settings.is_none() {
                errors.push(ParseError::const_error(
                    // this doesn't panic because we have ensured has_auth.is_some() in the line
                    // before
                auth.start,
                "used auth variable without declaring that the module requires auth. add @auth decorator at the start of the file."
            ))
            }
        }

        errors.extend(Self::check_reserved_words(sql));

        errors
    }

    pub fn new<'a>(
        front_matter: &FrontMatter,
        sql: Vec<SpanRef<'a, StatementSpan<'a>>>,
    ) -> CResult<'a, Statements> {
        let mut errors = Self::check_for_errors(front_matter, &sql);

        if errors.len() == 1 {
            // errors has at least one item
            Err(errors.pop().unwrap())?
        } else if errors.len() > 1 {
            Err(ParseError::Multiple(errors))?
        };

        let sql = sql
            .iter()
            .map(|span_ref| {
                span_ref
                    .0
                    .iter()
                    .map(|interp_ref| Interp::from(&*interp_ref))
                    .collect()
            })
            .collect();

        Ok(Self(sql))
    }
}
