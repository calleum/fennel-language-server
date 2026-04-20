use core::fmt;
use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Included},
    path::PathBuf,
};

use rowan::TextRange;

use crate::SyntaxToken;

// TODO: TextRange as key
#[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LSymbols(pub(crate) BTreeMap<u32, LSymbol>);

pub type Globals = Vec<(CompletionKind, Vec<&'static str>)>;

pub(crate) type RSymbols = Vec<RSymbol>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SuppressErrorKind {
    Unused,
    Unterminated,
    Undefined,
    AllUnexpected,
    Unexpected(crate::SyntaxKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ScopeExtend {
    Current,
    This(crate::SyntaxNode),
    Outer,
    #[allow(unused)]
    File, // useless?
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LSymbol {
    pub token: Token,
    pub scope: Scope,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RSymbol {
    pub(crate) token: Token,
    pub(crate) special: SpecialKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub text: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scope {
    pub kind: ScopeKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Value {
    pub kind: ValueKind,
    pub range: Option<TextRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeKind {
    Func,
    Lambda,
    Param,
    MacroParam,
    Local,
    Var,
    Let,
    WithOpen,
    IterValue,
    AccuValue,
    Global,
    Macro,
    Match,
    MatchTry,
    Catch,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueKind {
    Func,
    Param,
    MacroParam,
    Match,
    Number,
    String,
    Nil,
    Bool,
    SeqTable,
    KvTable,
    Macro,
    Module,
    Require(Option<PathBuf>),
    FileHandle,
    Unknown,
    Symbol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpecialKind {
    Normal,
    MacroWrap,
    MacroUnquote,
    HashArg,
}

impl LSymbol {
    pub(crate) fn contains_token(&self, r_symbol: &Token) -> bool {
        self.token.text == r_symbol.text && self.scope.range.contains_range(r_symbol.range)
    }
}

impl LSymbols {
    pub(crate) fn new(symbols: impl Iterator<Item = LSymbol>) -> Self {
        Self(symbols.map(|s| (s.token.range.start().into(), s)).collect())
    }

    pub(crate) fn range(&self, offset: u32) -> impl Iterator<Item = &LSymbol> {
        self.0.range((Included(0), Excluded(offset))).rev().map(|(_, s)| s)
    }

    pub(crate) fn nearest(&self, token: &Token) -> Option<&LSymbol> {
        self.range(token.range.start().into()).find(|l_symbol| l_symbol.contains_token(token))
    }

    pub(crate) fn get(&self, start: u32) -> Option<&LSymbol> {
        self.0.get(&start)
    }
}

impl From<SyntaxToken> for Token {
    fn from(token: SyntaxToken) -> Self {
        Token { text: token.text().to_string(), range: token.text_range() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompletionKind {
    Keyword,
    Func,
    Module,
    Field,
    Operator,
    Var,
}

pub struct AstDocumentSymbol {
    /// The name of this symbol.
    pub name: String,
    /// More detail for this symbol, e.g the signature of a function. If not provided the
    /// name is used.
    pub detail: Option<String>,
    /// The kind of this symbol.
    pub kind: ValueKind,
    /// The range enclosing this symbol not including leading/trailing whitespace but everything else
    /// like comments. This information is typically used to determine if the the clients cursor is
    /// inside the symbol to reveal in the symbol in the UI.
    pub range: TextRange,
    /// The range that should be selected and revealed when this symbol is being picked, e.g the name of a function.
    /// Must be contained by the the `range`.
    pub selection_range: TextRange,
    /// Children of this symbol, e.g. properties of a class.
    pub children: Option<Vec<AstDocumentSymbol>>,
}

impl AstDocumentSymbol {
    pub fn new(
        name: String,
        detail: Option<String>,
        kind: ValueKind,
        range: TextRange,
        selection_range: TextRange,
        children: Option<Vec<AstDocumentSymbol>>,
    ) -> Self {
        Self { name, detail, kind, range, selection_range, children }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstSymbolInformation {
    /// The name of this symbol.
    pub name: String,

    /// The kind of this symbol.
    pub kind: ValueKind,

    /// The location of this symbol.
    pub location: TextRange,
}

impl AstSymbolInformation {
    pub fn new(name: String, kind: ValueKind, location: TextRange) -> Self {
        Self { name, kind, location }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Definition {
    Symbol(LSymbol, bool),
    FileSymbol(PathBuf, LSymbol),
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    ConvertToColonString(String),
    ConvertToQuoteString(String),
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Func => "function",
            Self::Param => "parameter",
            Self::Match => "match pattern",
            Self::Number => "number",
            Self::String => "string",
            Self::Nil => "nil",
            Self::Bool => "bool",
            Self::SeqTable => "sequential table",
            Self::KvTable => "key/value table",
            Self::Macro => "macro",
            Self::MacroParam => "macro parameter",
            Self::Module => "module",
            Self::Require(_) => "require",
            Self::FileHandle => "file handle",
            Self::Symbol => "symbol",
            Self::Unknown => "(lsp:unknown)",
        };
        write!(f, "{}", name)
    }
}
