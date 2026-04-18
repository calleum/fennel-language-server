use rowan::TextRange;
use thiserror::Error;

use crate::{models::ValueKind, syntax::SyntaxKind};

#[derive(Error, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorKind {
    #[error("unexpected token {0:?}")]
    Unexpected(SyntaxKind),
    #[error("unexpected varargs")]
    UnexpectedVarargs,
    #[error("multiple varargs found")]
    MultiVarargs,
    #[error("unexpected end of file")]
    UnexpectedEof,
    #[error("empty list is not allowed")]
    EmptyList,
    #[error("dismatched structure")]
    Dismatched,
    #[error("unterminated {0:?}")]
    Unterminated(SyntaxKind),
    #[error("undefined symbol: {0}")]
    Undefined(String),
    #[error("unused symbol")]
    Unused,
    #[error("global symbol conflict")]
    GlobalConflict,
    #[error("missing whitespace")]
    MissingWhitespace,
    #[error("macro whitespace error")]
    MacroWhitespace,
    #[error("invalid symbol")]
    InvalidSymbol,
    #[error("method not allowed in this context")]
    MethodNotAllowed,
    #[error("field and method both present, not allowed")]
    FieldAndMethodNotAllowed,
    #[error("literal call of {0:?}")]
    LiteralCall(ValueKind),
    #[error("direct call of {0:?}")]
    DirectCall(ValueKind),
    #[error("multiple catch clauses not allowed")]
    MultiCatch,
    #[error("catch must be the last clause")]
    CatchNotLast,
    #[error("deprecated since {0}: {1}")]
    Deprecated(&'static str, &'static str),
}

#[derive(Error, Debug, Clone, PartialEq, Eq, Hash)]
#[error("{kind} at {range:?}")]
pub struct Error {
    pub range: TextRange,
    pub kind: ErrorKind,
}

impl Error {
    pub(crate) fn new(range: TextRange, kind: ErrorKind) -> Self {
        Error { range, kind }
    }
}
