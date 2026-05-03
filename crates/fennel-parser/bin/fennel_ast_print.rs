use std::{collections::HashSet, env, fs, process};

use fennel_parser::ast::nodes::*;
use fennel_parser::{SyntaxKind, SyntaxNode, parse};
use rowan::{NodeOrToken, ast::AstNode};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: fennel-ast-print <file.fnl>");
        process::exit(1);
    }

    let source = match fs::read_to_string(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", args[1], e);
            process::exit(1);
        }
    };

    let ast = parse(source.chars(), HashSet::new());

    // Print any parse errors to stderr.
    let mut has_errors = false;
    for err in ast.errors() {
        eprintln!("error: {:?} at {:?}", err.kind, err.range);
        has_errors = true;
    }

    // Reconstruct the Root AST node and walk it.
    let root_syntax = SyntaxNode::new_root(ast.root);
    let root = Root::cast(root_syntax).expect("parse output must be a Root node");
    print_ast_node(root.syntax(), 0);

    if has_errors {
        process::exit(1);
    }
}

/// Walk the syntax tree, using the typed AST `can_cast` checks for
/// descriptive labels. Falls back to the SyntaxKind name for anything
/// not covered by the typed AST nodes.
fn print_ast_node(node: &SyntaxNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let label = typed_label(node);

    let children: Vec<_> = node.children_with_tokens().collect();
    let has_children = children.iter().any(|e| matches!(e, NodeOrToken::Node(_)));

    if !has_children {
        let text = node.text().to_string();
        println!("{}{} @ {:?} {}", indent, label, node.text_range(), truncate(&text, 80));
        return;
    }

    println!("{}{}", indent, label);
    for child in &children {
        match child {
            NodeOrToken::Node(n) => print_ast_node(n, depth + 1),
            NodeOrToken::Token(t) => {
                let kind: SyntaxKind = t.kind();
                if kind == SyntaxKind::WHITESPACE {
                    continue;
                }
                println!(
                    "{}  {} @ {:?} {}",
                    indent,
                    token_label(kind),
                    t.text_range(),
                    truncate(t.text(), 80)
                );
            }
        }
    }
}

/// Check each typed AST node type via `can_cast`. Returns the human-readable
/// Rust type name for the first match, or falls back to the raw SyntaxKind.
fn typed_label(node: &SyntaxNode) -> &'static str {
    // Each AST node maps to a distinct SyntaxKind, so order doesn't matter.
    // We use the typed AST node types as the source of truth.
    macro_rules! check {
        ($($ty:ident),* $(,)?) => {
            $(if $ty::can_cast(node.kind()) {
                return stringify!($ty);
            })*
        };
    }

    check!(
        Root,
        Sexp,
        Atom,
        Literal,
        List,
        SubList,
        Operation,
        Func,
        Lambda,
        Var,
        Set,
        Tset,
        Local,
        Global,
        Let,
        Match,
        MatchTry,
        Catch,
        If,
        When,
        For,
        Each,
        Do,
        Thread,
        Doto,
        Values,
        PickValues,
        WithOpen,
        Icollect,
        Fcollect,
        Collect,
        Accumulate,
        ImportMacros,
        RequireMacros,
        PickArgs,
        Macro,
        Macros,
        EvalCompiler,
        SymbolCall,
        Lua,
        Macrodebug,
        IntoClause,
        UntilClause,
        MacroQuote,
        LeftSymbol,
        RightSymbol,
        LeftRightSymbol,
        LeftOrRightSymbol,
        KvTable,
    );

    // Fall back to the SyntaxKind Debug name for internal/untyped nodes.
    let kind: SyntaxKind = node.kind();
    let name = format!("{:?}", kind).trim_start_matches("N_").replace('_', " ");
    Box::leak(name.into_boxed_str())
}

fn token_label(kind: SyntaxKind) -> &'static str {
    use SyntaxKind::*;
    match kind {
        L_PAREN => "LParen",
        R_PAREN => "RParen",
        L_BRACKET => "LBracket",
        R_BRACKET => "RBracket",
        L_BRACE => "LBrace",
        R_BRACE => "RBrace",
        COMMA => "Comma",
        COLON => "Colon",
        VARARG => "VarArg",
        HASHFN => "HashFn",
        CAPTURE => "Capture",
        LENGTH => "Length",
        THREAD => "ThreadArrow",
        QUESTION => "Question",
        BACKTICK => "Backtick",
        SYMBOL => "Symbol",
        SYMBOL_FIELD => "SymbolField",
        SYMBOL_METHOD => "SymbolMethod",
        COMMENT => "Comment",
        ERROR => "Error",
        END => "End",
        _ => Box::leak(format!("{:?}", kind).into_boxed_str()),
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return format!("{:?}", s);
    }
    let end = s.char_indices().take(max_chars).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    format!("{:?}...", &s[..end])
}
