use nom::IResult;
use thiserror::Error;

// TODO add a way to specify hints on what the user should do
#[derive(Error, Debug, Clone)]
pub enum ParseError<'a> {
    #[error("failed at {0:?}")]
    Multiple(Vec<ParseError<'a>>),
    #[error("Parser failed at {0}")]
    NomError(&'a str, nom::error::ErrorKind),
    #[error("Parser failed at {0} due to {1}")]
    ErrorKind(&'a str, ErrorKind),
    #[error("Checking failed at {0} due to {1}")]
    IrErrorKind(&'a str, IrErrorKind),
}

#[derive(Error, Debug, Clone)]
pub enum ErrorKind {
    #[error("{0}")]
    ConstError(&'static str),
    #[error("undefined parameter {0}")]
    UndefinedParameterError(String),
    #[error("argument {0} in function {0} does not exist")]
    UndefinedArgumentError(String, String),
}

#[derive(Error, Debug, Clone)]
pub enum IrErrorKind {
    #[error("{0}")]
    ConstError(&'static str),
    #[error("{0} is a reserved words")]
    ReservedWordError(String),
    #[error("function {0} does not exist")]
    UndefinedFunctionError(String),
    #[error("this module expects {0} arguments not {1} arguments")]
    WrongNumberArgumentsError(usize, usize),
}

impl<'a> ParseError<'a> {
    pub fn const_error(input: &'a str, reason: &'static str) -> ParseError<'a> {
        ParseError::ErrorKind(input, ErrorKind::ConstError(reason))
    }
    pub fn error_kind(input: &'a str, kind: ErrorKind) -> ParseError<'a> {
        ParseError::ErrorKind(input, kind)
    }
}

impl<'a> nom::error::ParseError<&'a str> for ParseError<'a> {
    fn from_error_kind(input: &'a str, kind: nom::error::ErrorKind) -> Self {
        ParseError::NomError(input, kind)
    }

    fn append(_input: &'a str, _kind: nom::error::ErrorKind, other: Self) -> Self {
        other
    }
}

/// Parse Result
pub type PResult<'a, O> = IResult<&'a str, O, ParseError<'a>>;

/// Codegen Result
pub type CResult<'a, O> = std::result::Result<O, ParseError<'a>>;
