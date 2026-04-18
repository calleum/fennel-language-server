mod ast;
mod errors;
mod lexer;
mod parser;
mod syntax;

use std::collections::HashSet;

pub use ast::{models, Action, Ast, Definition};
pub use errors::{Error, ErrorKind};
pub use rowan::TextRange;
pub(crate) use syntax::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FennelLanguage {}
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

type SyntaxNode = rowan::SyntaxNode<FennelLanguage>;
type SyntaxToken = rowan::SyntaxToken<FennelLanguage>;
type SyntaxElement = rowan::SyntaxElement<FennelLanguage>;

pub fn parse(
    text: impl Iterator<Item = char>,
    globals: HashSet<String>,
) -> ast::Ast {
    let parsed = parser::Parser::new(Box::new(text)).parse();
    ast::Ast::new(parsed.green_node, parsed.errors, globals)
}

#[test]
fn learn_cst_structure() {
    let code = "(local x 10) (+ x 5)"; // This is your "small file"
    let globals = std::collections::HashSet::new();

    // Parse the string into your Ast
    let ast = parse(code.chars(), globals);
    // Get the Rowan Root Node
    let root = SyntaxNode::new_root(ast.root.clone());

    // Iterate through every single piece of the syntax tree
    for node in root.descendants() {
        println!(
            "Kind: {:?}, Range: {:?}, Text: '{}'",
            node.kind(),
            node.text_range(),
            node.text()
        );
    }
}
