pub mod ast;
mod builtins;
mod errors;
mod lexer;
mod parser;
mod syntax;

use std::{collections::HashSet, sync::Arc};

pub use ast::{Action, Ast, AstDocumentSymbol, AstSymbolInformation, Definition, models};
pub use errors::{Error, ErrorKind};
pub use rowan::TextRange;
pub use syntax::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FennelLanguage {}
impl rowan::Language for FennelLanguage {
    type Kind = syntax::SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= syntax::SyntaxKind::ROOT as u16);
        raw.0.into()
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<FennelLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<FennelLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<FennelLanguage>;

pub fn parse(text: impl Iterator<Item = char>, globals: HashSet<String>) -> Ast {
    parse_shared(text, Arc::new(globals))
}

pub fn parse_shared(text: impl Iterator<Item = char>, globals: Arc<HashSet<String>>) -> Ast {
    let parsed = parser::Parser::new(text).parse();
    Ast::new_shared(parsed.green_node, parsed.errors, globals)
}
