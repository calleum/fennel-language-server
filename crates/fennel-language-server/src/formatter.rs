//! Source formatter built on the parse tree.
//!
//! Formatting works from the CST rather than the raw text so comments, string
//! contents, and delimiter structure survive. Input the parser cannot make
//! sense of is declined outright — a formatter that mangles code it failed to
//! understand is worse than no formatter at all.
//!
//! Layout rules:
//!   - a form that fits on one line and holds no comment stays on one line
//!   - otherwise the delimiters sit on their own lines and the body is indented
//!   - line breaks already present in the source are preserved
//!   - comments are never dropped

use fennel_parser::SyntaxKind::{self, *};
use fennel_parser::SyntaxNode;

/// Indentation width, in spaces.
const INDENT: usize = 2;

/// Widest single-line form. Longer forms are broken up.
const MAX_SINGLE_LINE: usize = 72;

/// Formats `source`, or returns `None` if it is not well-formed enough to
/// reformat safely.
pub fn format(source: &str) -> Option<String> {
    let ast = fennel_parser::parse(source.chars(), Default::default());

    // `errors()` mixes syntax errors with undefined-name diagnostics. Only
    // the former mean the tree cannot be trusted for layout.
    if ast.errors().any(|e| {
        matches!(
            e.kind,
            fennel_parser::ErrorKind::Unexpected(_)
                | fennel_parser::ErrorKind::Dismatched
                | fennel_parser::ErrorKind::UnexpectedEof
        )
    }) {
        return None;
    }

    let root = SyntaxNode::new_root(ast.root.clone());
    let mut out = String::with_capacity(source.len());
    Formatter { out: &mut out }.root(&root);

    // Normalize the edges: no leading blank lines, exactly one trailing newline.
    let mut text = out.trim_start_matches('\n').to_string();
    while text.ends_with([' ', '\t', '\n']) {
        text.pop();
    }
    if !text.is_empty() {
        text.push('\n');
    }
    Some(text)
}

/// Nodes that carry their own delimiters.
/// Nodes that carry their own delimiters, and so must be re-emitted rather
/// than descended into.
fn is_delimited(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        N_LIST
            | N_SEQ_TABLE
            | N_NV_PAIR_TABLE
            | N_PARAM_TABLE
            | N_KV_TABLE
            | N_ASSIGN_PATTERN_KV_TABLE
            | N_MATCH_PATTERN_KV_TABLE
            | N_IMPORT_MACROS_KV_TABLE
    )
}

/// Opening and closing delimiters for a bracketed node kind.
fn delimiters(kind: SyntaxKind) -> (&'static str, &'static str) {
    match kind {
        N_KV_TABLE
        | N_ASSIGN_PATTERN_KV_TABLE
        | N_MATCH_PATTERN_KV_TABLE
        | N_IMPORT_MACROS_KV_TABLE => ("{", "}"),
        N_SEQ_TABLE | N_PARAM_TABLE | N_NV_PAIR_TABLE => ("[", "]"),
        _ => ("(", ")"),
    }
}

fn is_delimiter_token(kind: SyntaxKind) -> bool {
    matches!(kind, L_PAREN | R_PAREN | L_BRACKET | R_BRACKET | L_BRACE | R_BRACE)
}

/// Leaf tokens that must not be descended into: their text is data, not
/// structure.
fn is_leaf_token(kind: SyntaxKind) -> bool {
    matches!(kind, SYMBOL | SyntaxKind::QUOTE_STRING | SyntaxKind::COLON_STRING | INTEGER | FLOAT)
        || is_delimiter_token(kind)
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..(level * INDENT) {
        out.push(' ');
    }
}

/// True when a space should separate the text about to be appended from what
/// is already on the line. A line ending in an opening delimiter is a call
/// position, where a space would be wrong.
fn needs_space(line: &str) -> bool {
    !line.is_empty() && !line.ends_with([' ', '(', '{', '['])
}

struct Formatter<'a> {
    out: &'a mut String,
}

impl Formatter<'_> {
    /// Renders a form's body into `line`, which callers flush to the output.
    fn body(&mut self, node: &SyntaxNode, level: usize, line: &mut String) {
        for element in node.children_with_tokens() {
            match element {
                rowan::NodeOrToken::Token(t) => self.token(&t, level, line),
                rowan::NodeOrToken::Node(n) => {
                    if is_delimited(n.kind()) {
                        self.form(&n, level, line);
                    } else if is_leaf_token(n.kind()) {
                        self.append_text(&n.text().to_string(), line);
                    } else {
                        // Transparent wrapper: recurse so keywords and their
                        // arguments are seen individually.
                        self.body(&n, level, line);
                    }
                }
            }
        }
    }

    fn token(&mut self, token: &fennel_parser::SyntaxToken, level: usize, line: &mut String) {
        match token.kind() {
            // Delimiters are re-emitted structurally.
            k if is_delimiter_token(k) => {}
            WHITESPACE => {
                // Preserve the author's line breaks, but never start a line
                // with one.
                if token.text().contains('\n') && !line.is_empty() {
                    self.flush(line, level);
                }
            }
            COMMENT => {
                self.flush(line, level);
                self.out.push('\n');
                indent(self.out, level);
                self.out.push_str(token.text().trim());
            }
            _ => self.append_text(token.text(), line),
        }
    }

    fn append_text(&mut self, text: &str, line: &mut String) {
        if needs_space(line) {
            line.push(' ');
        }
        line.push_str(text);
    }

    /// Emits `line` as one output row at `level`, then clears it.
    fn flush(&mut self, line: &mut String, level: usize) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            return;
        }
        self.out.push('\n');
        indent(self.out, level);
        self.out.push_str(trimmed);
        line.clear();
    }

    /// Renders a delimited form, either inline or broken across lines.
    fn form(&mut self, node: &SyntaxNode, level: usize, line: &mut String) {
        // Column where this form will start, used for the width budget.
        let column = level * INDENT + line.chars().count() + usize::from(needs_space(line));
        if let Some(inline) = self.inline(node, column) {
            self.append_text(&inline, line);
            return;
        }
        // Multi-line: the closing delimiter returns to the parent's level.
        let (opening, closing) = delimiters(node.kind());

        self.append_text(opening, line);
        self.flush(line, level);

        let child_level = level + 1;
        let mut child = String::new();
        self.body(node, child_level, &mut child);
        self.flush(&mut child, child_level);

        // The closing delimiter is dedented to this form's own level,
        // directly after whatever the body left behind.
        self.out.push('\n');
        indent(self.out, level);
        self.out.push_str(closing);
    }

    /// Attempts the single-line rendering, or `None` if the form is too long
    /// or contains a comment (which must stay on its own line).
    fn inline(&self, node: &SyntaxNode, column: usize) -> Option<String> {
        let (opening, closing) = delimiters(node.kind());

        let mut body = String::new();
        let mut bail = false;
        self.inline_into(node, &mut body, &mut bail);
        // A comment anywhere inside forces the multi-line layout.
        if bail {
            return None;
        }
        let body = body.trim();
        if body.is_empty() {
            return None;
        }

        let rendered = format!("{opening}{body}{closing}");
        // Measure against where the form will actually start on the line, so
        // a short nested form inside a long one still breaks.
        if column + rendered.chars().count() > MAX_SINGLE_LINE {
            return None;
        }
        Some(rendered)
    }

    /// Builds the inline rendering into `out`. A comment anywhere forces the
    /// multi-line layout, signalled by `bail`.
    fn inline_into(&self, node: &SyntaxNode, out: &mut String, bail: &mut bool) {
        for element in node.children_with_tokens() {
            match element {
                rowan::NodeOrToken::Token(t) => match t.kind() {
                    k if is_delimiter_token(k) => {}
                    WHITESPACE => {}
                    COMMENT => *bail = true,
                    _ => {
                        if needs_space(out) {
                            out.push(' ');
                        }
                        out.push_str(t.text());
                    }
                },
                rowan::NodeOrToken::Node(n) => {
                    if is_delimited(n.kind()) {
                        // Only genuinely delimited nodes add punctuation.
                        let opening = delimiters(n.kind()).0;
                        let closing = delimiters(n.kind()).1;
                        if needs_space(out) {
                            out.push(' ');
                        }
                        out.push_str(opening);
                        self.inline_into(&n, out, bail);
                        out.push_str(closing);
                    } else if is_leaf_token(n.kind()) {
                        if needs_space(out) {
                            out.push(' ');
                        }
                        out.push_str(&n.text().to_string());
                    } else {
                        self.inline_into(&n, out, bail);
                    }
                }
            }
        }
    }

    /// Formats the top level: one top-level form or comment per row.
    fn root(&mut self, root: &SyntaxNode) {
        for element in root.children_with_tokens() {
            match element {
                rowan::NodeOrToken::Token(t) => {
                    if t.kind() == COMMENT {
                        if !self.out.is_empty() && !self.out.ends_with('\n') {
                            self.out.push('\n');
                        }
                        self.out.push_str(t.text().trim_end());
                        self.out.push('\n');
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    let mut line = String::new();
                    self.body(&n, 0, &mut line);
                    self.flush(&mut line, 0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    /// The formatter must be idempotent: re-formatting formatted code is a
    /// no-op. Without this, format-on-save churns the buffer forever.
    #[test]
    fn formatting_is_idempotent() {
        let cases = [
            "(local x 1)",
            "(fn greet [name]\n  (print name))\n{: greet}",
            "{:a 1 :b 2}",
            "(local x (foo (bar 1) (baz 2)))",
            "-- comment\n(print 1)",
            "(local s \"a  b\")",
            "[1 2 3]",
        ];
        for case in cases {
            let once = format(case).unwrap_or_else(|| panic!("declined: {case:?}"));
            let twice = format(&once).unwrap_or_else(|| panic!("declined on 2nd pass: {once:?}"));
            assert_eq!(once, twice, "not idempotent for {case:?}");
        }
    }

    /// Formatting must not change what the program means, so the symbol table
    /// has to survive.
    #[test]
    fn formatting_preserves_definitions() {
        let case = "(fn add [a b] (+ a b))\n{: add}\n(fn sub [a b] (- a b))\n{: sub}";
        let out = format(case).expect("should format");
        let defs = |src: &str| {
            let ast = fennel_parser::parse(src.chars(), Default::default());
            ast.all_l_symbols()
                .map(|s| s.token.text.clone())
                .collect::<std::collections::HashSet<_>>()
        };
        assert_eq!(defs(case), defs(&out));
    }

    /// Unparseable input must be declined rather than mangled.
    #[test]
    fn broken_input_is_declined() {
        assert!(format("(local x").is_none());
        assert!(format("(fn foo [a b] (print a").is_none());
        assert!(format("}}}}").is_none());
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(format("(local   x    1)").as_deref(), Some("(local x 1)\n"));
    }

    #[test]
    fn ensures_single_trailing_newline() {
        for input in ["(local x 1)", "(local x 1)\n\n\n"] {
            let out = format(input).expect("should format");
            assert!(out.ends_with('\n'), "missing trailing newline: {out:?}");
            assert!(!out.ends_with("\n\n"), "extra trailing newline: {out:?}");
        }
    }

    /// Documents the stable output for common shapes so regressions show up in
    /// the diff.
    #[test]
    fn layout_is_stable_for_common_shapes() {
        let cases: &[(&str, &str)] = &[
            ("(local x 1)", "(local x 1)\n"),
            ("(local x (foo 1))", "(local x (foo 1))\n"),
            ("{ :a 1 :b 2 }", "{:a 1 :b 2}\n"),
            ("[1 2 3]", "[1 2 3]\n"),
        ];
        for (input, expected) in cases {
            assert_eq!(format(input).as_deref(), Some(*expected), "input {input:?}");
        }
    }

    /// Long forms break across lines, with the closing delimiter dedented back
    /// to the opening delimiter's level.
    #[test]
    fn long_forms_break_across_lines() {
        let input =
            "(local result (some-function-name arg-one arg-two arg-three arg-four arg-five))";
        let out = format(input).expect("should format");
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() > 1, "expected multi-line, got {out:?}");
        assert_eq!(lines[0], "(");
        for line in &lines[1..lines.len() - 1] {
            assert!(line.starts_with("  "), "body not indented: {line:?}");
        }
        assert_eq!(lines[lines.len() - 1], ")");
    }

    /// Dropping a comment would silently delete user content.
    #[test]
    fn comments_are_preserved() {
        let out = format("-- a note\n(print 1)").expect("should format");
        assert!(out.contains("-- a note"), "comment lost: {out:?}");
    }

    /// String contents are opaque.
    #[test]
    fn string_contents_are_not_reformatted() {
        let out = format(r#"(print "  keep   me  ")"#).expect("should format");
        assert!(out.contains(r#""  keep   me  ""#), "string mangled: {out:?}");
    }
}
