use rowan::{TextRange, TextSize};

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroExpansionInfo {
    pub signature: &'static str,
    pub docstring: &'static str,
    pub expansion: String,
    pub call_range: TextRange,
}

fn is_macro_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::N_THREAD
            | SyntaxKind::N_DOTO
            | SyntaxKind::N_WHEN
            | SyntaxKind::N_ICOLLECT
            | SyntaxKind::N_COLLECT
            | SyntaxKind::N_FCOLLECT
    )
}

fn is_macro_keyword_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::THREAD
            | SyntaxKind::KEYWORD_DOTO
            | SyntaxKind::KEYWORD_WHEN
            | SyntaxKind::KEYWORD_ICOLLECT
            | SyntaxKind::KEYWORD_COLLECT
            | SyntaxKind::KEYWORD_FCOLLECT
    )
}

fn extract_call(call_fn: &SyntaxNode) -> Option<(String, Vec<String>)> {
    if let Some(list) = call_fn.children().find(|n| n.kind() == SyntaxKind::N_LIST) {
        let sublist = list.children().find(|n| n.kind() == SyntaxKind::N_SUBLIST)?;

        if let Some(sym_call) = sublist.children().find(|n| n.kind() == SyntaxKind::N_SYMBOL_CALL) {
            let callee = sym_call
                .children()
                .find(|n| n.kind() != SyntaxKind::N_ARGS)
                .map(|n| n.text().to_string().trim().to_string())
                .or_else(|| sym_call.first_token().map(|t| t.text().trim().to_string()))?;

            let mut args = Vec::new();
            if let Some(args_node) = sym_call
                .children()
                .find(|n| n.kind() == SyntaxKind::N_ARGS)
                .or_else(|| sublist.children().find(|n| n.kind() == SyntaxKind::N_ARGS))
            {
                for sexp in args_node.children().filter(|n| n.kind() == SyntaxKind::N_SEXP) {
                    args.push(sexp.text().to_string().trim().to_string());
                }
            }
            return Some((callee, args));
        }

        if let Some(op) = sublist.children().find(|n| n.kind() == SyntaxKind::N_OPERATION) {
            let callee = op.first_token().map(|t| t.text().trim().to_string())?;
            let mut args = Vec::new();
            if let Some(args_node) = op.children().find(|n| n.kind() == SyntaxKind::N_ARGS) {
                for sexp in args_node.children().filter(|n| n.kind() == SyntaxKind::N_SEXP) {
                    args.push(sexp.text().to_string().trim().to_string());
                }
            }
            return Some((callee, args));
        }

        let first_child = sublist.children().next()?;
        let callee = first_child.text().to_string().trim().to_string();
        let mut args = Vec::new();
        if let Some(args_node) = sublist.children().find(|n| n.kind() == SyntaxKind::N_ARGS) {
            for sexp in args_node.children().filter(|n| n.kind() == SyntaxKind::N_SEXP) {
                args.push(sexp.text().to_string().trim().to_string());
            }
        }
        return Some((callee, args));
    }

    let callee = call_fn.text().to_string().trim().to_string();
    if !callee.is_empty() { Some((callee, Vec::new())) } else { None }
}

fn expand_specific_macro(
    macro_node: &SyntaxNode,
    call_range: TextRange,
) -> Option<MacroExpansionInfo> {
    match macro_node.kind() {
        SyntaxKind::N_THREAD => {
            let arrow = macro_node.first_token()?.text().to_string();
            let target_node = macro_node.children().find(|n| n.kind() == SyntaxKind::N_SEXP)?;
            let target_str = target_node.text().to_string().trim().to_string();
            let calls: Vec<(String, Vec<String>)> = macro_node
                .children()
                .filter(|n| n.kind() == SyntaxKind::N_CALL_FN)
                .filter_map(|n| extract_call(&n))
                .collect();

            let expansion = match arrow.as_str() {
                "->" => {
                    let mut acc = target_str;
                    for (callee, args) in calls {
                        if args.is_empty() {
                            acc = format!("({callee} {acc})");
                        } else {
                            acc = format!("({callee} {acc} {})", args.join(" "));
                        }
                    }
                    acc
                }
                "->>" => {
                    let mut acc = target_str;
                    for (callee, args) in calls {
                        if args.is_empty() {
                            acc = format!("({callee} {acc})");
                        } else {
                            acc = format!("({callee} {} {acc})", args.join(" "));
                        }
                    }
                    acc
                }
                "-?>" => {
                    let mut acc = target_str;
                    for (callee, args) in calls {
                        let call_expr = if args.is_empty() {
                            format!("({callee} _val)")
                        } else {
                            format!("({callee} _val {})", args.join(" "))
                        };
                        acc = format!("(let [_val {acc}] (if (~= nil _val) {call_expr}))");
                    }
                    acc
                }
                "-?>>" => {
                    let mut acc = target_str;
                    for (callee, args) in calls {
                        let call_expr = if args.is_empty() {
                            format!("({callee} _val)")
                        } else {
                            format!("({callee} {} _val)", args.join(" "))
                        };
                        acc = format!("(let [_val {acc}] (if (~= nil _val) {call_expr}))");
                    }
                    acc
                }
                _ => return None,
            };

            let (signature, docstring) = match arrow.as_str() {
                "->" => (
                    "(-> val ...)",
                    "Threads the expression through any number of forms. Each form is treated as a function call, and the result of the previous expression is inserted as the first argument.",
                ),
                "->>" => (
                    "(->> val ...)",
                    "Threads the expression through any number of forms. Each form is treated as a function call, and the result of the previous expression is inserted as the last argument.",
                ),
                "-?>" => (
                    "(-?> val ...)",
                    "Threads the expression through any number of forms, returning nil if any step evaluates to nil.",
                ),
                "-?>>" => (
                    "(-?>> val ...)",
                    "Threads the expression through any number of forms, returning nil if any step evaluates to nil.",
                ),
                _ => ("(-> val ...)", ""),
            };

            Some(MacroExpansionInfo { signature, docstring, expansion, call_range })
        }
        SyntaxKind::N_DOTO => {
            let target_node = macro_node.children().find(|n| n.kind() == SyntaxKind::N_SEXP)?;
            let target_str = target_node.text().to_string().trim().to_string();
            let calls: Vec<(String, Vec<String>)> = macro_node
                .children()
                .filter(|n| n.kind() == SyntaxKind::N_CALL_FN)
                .filter_map(|n| extract_call(&n))
                .collect();
            let calls_str: Vec<String> = calls
                .into_iter()
                .map(|(callee, args)| {
                    if args.is_empty() {
                        format!("({callee} _val)")
                    } else {
                        format!("({callee} _val {})", args.join(" "))
                    }
                })
                .collect();

            let expansion = if calls_str.is_empty() {
                format!("(let [_val {target_str}] _val)")
            } else {
                format!("(let [_val {target_str}] {} _val)", calls_str.join(" "))
            };

            Some(MacroExpansionInfo {
                signature: "(doto val ...)",
                docstring: "Evaluate val and splice it into the first argument of subsequent forms.",
                expansion,
                call_range,
            })
        }
        SyntaxKind::N_WHEN => {
            let cond_node = macro_node.children().find(|n| n.kind() == SyntaxKind::N_COND)?;
            let cond_str = cond_node.text().to_string().trim().to_string();
            let body_node = macro_node.children().find(|n| n.kind() == SyntaxKind::N_BODY);
            let body_sexps: Vec<String> = body_node
                .map(|b| {
                    b.children()
                        .filter(|n| n.kind() == SyntaxKind::N_SEXP)
                        .map(|s| s.text().to_string().trim().to_string())
                        .collect()
                })
                .unwrap_or_default();

            let expansion = if body_sexps.is_empty() {
                format!("(if {cond_str} nil)")
            } else {
                format!("(if {cond_str} (do {}))", body_sexps.join(" "))
            };

            Some(MacroExpansionInfo {
                signature: "(when condition ...)",
                docstring: "Evaluate condition; if truthy, evaluate body forms and return the value of the last form.",
                expansion,
                call_range,
            })
        }
        SyntaxKind::N_ICOLLECT => {
            let table_node =
                macro_node.children().find(|n| n.kind() == SyntaxKind::N_ICOLLECT_TABLE)?;
            let table_str = table_node.text().to_string().trim().to_string();
            let body_sexps: Vec<String> = macro_node
                .children()
                .filter(|n| n.kind() == SyntaxKind::N_SEXP)
                .map(|s| s.text().to_string().trim().to_string())
                .collect();
            let body_str = body_sexps.join(" ");

            let expansion = format!(
                "(let [tbl_ []] (each {table_str} (tset tbl_ (+ (length tbl_) 1) {body_str})) tbl_)"
            );

            Some(MacroExpansionInfo {
                signature: "(icollect [_ v iter] ...)",
                docstring: "Collect sequential values into a new sequential table.",
                expansion,
                call_range,
            })
        }
        SyntaxKind::N_COLLECT => {
            let table_node =
                macro_node.children().find(|n| n.kind() == SyntaxKind::N_COLLECT_TABLE)?;
            let table_str = table_node.text().to_string().trim().to_string();
            let body_sexps: Vec<String> = macro_node
                .children()
                .filter(|n| n.kind() == SyntaxKind::N_SEXP)
                .map(|s| s.text().to_string().trim().to_string())
                .collect();
            let body_str = body_sexps.join(" ");

            let expansion = format!(
                "(let [tbl_ {{}}] (each {table_str} (let [(k_ v_) {body_str}] (tset tbl_ k_ v_))) tbl_)"
            );

            Some(MacroExpansionInfo {
                signature: "(collect [k v iter] ...)",
                docstring: "Collect key/value pairs into a new table.",
                expansion,
                call_range,
            })
        }
        SyntaxKind::N_FCOLLECT => {
            let table_node =
                macro_node.children().find(|n| n.kind() == SyntaxKind::N_FCOLLECT_TABLE)?;
            let table_str = table_node.text().to_string().trim().to_string();
            let body_sexps: Vec<String> = macro_node
                .children()
                .filter(|n| n.kind() == SyntaxKind::N_SEXP)
                .map(|s| s.text().to_string().trim().to_string())
                .collect();
            let body_str = body_sexps.join(" ");

            let expansion = format!(
                "(let [tbl_ []] (for {table_str} (tset tbl_ (+ (length tbl_) 1) {body_str})) tbl_)"
            );

            Some(MacroExpansionInfo {
                signature: "(fcollect [i start stop step] ...)",
                docstring: "Collect values into a sequential table by iterating over a numeric range.",
                expansion,
                call_range,
            })
        }
        _ => None,
    }
}

pub fn expand_macro_node(node: &SyntaxNode) -> Option<MacroExpansionInfo> {
    if node.kind() == SyntaxKind::N_LIST {
        let sublist = node.children().find(|n| n.kind() == SyntaxKind::N_SUBLIST)?;
        let macro_child = sublist.children().find(|n| is_macro_kind(n.kind()))?;
        return expand_specific_macro(&macro_child, node.text_range());
    }

    if is_macro_kind(node.kind()) {
        let list = node.ancestors().find(|n| n.kind() == SyntaxKind::N_LIST)?;
        return expand_specific_macro(node, list.text_range());
    }

    None
}

pub fn expand_macro_at_offset(root: &SyntaxNode, offset: u32) -> Option<MacroExpansionInfo> {
    let token = root.token_at_offset(TextSize::from(offset)).right_biased()?;

    if (token.kind() == SyntaxKind::L_PAREN || token.kind() == SyntaxKind::R_PAREN)
        && let Some(list) = token.parent_ancestors().find(|n| n.kind() == SyntaxKind::N_LIST)
        && let Some(info) = expand_macro_node(&list)
    {
        return Some(info);
    }

    for node in token.parent_ancestors() {
        if is_macro_kind(node.kind()) {
            let list = node.ancestors().find(|n| n.kind() == SyntaxKind::N_LIST)?;
            return expand_macro_node(&list);
        }
    }

    None
}

pub fn is_macro_token(token: &SyntaxToken) -> bool {
    if is_macro_keyword_token(token.kind()) {
        return true;
    }
    if token.kind() == SyntaxKind::L_PAREN
        && let Some(list) = token.parent_ancestors().find(|n| n.kind() == SyntaxKind::N_LIST)
    {
        return expand_macro_node(&list).is_some();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::parse;

    #[test]
    fn test_thread_first_expansion() {
        let code = "(-> x (foo))";
        let ast = parse(code.chars(), HashSet::new());
        let root = SyntaxNode::new_root(ast.root.clone());
        let info = expand_macro_at_offset(&root, 1).expect("expected macro info");
        assert_eq!(info.expansion, "(foo x)");
        assert_eq!(info.signature, "(-> val ...)");
    }

    #[test]
    fn test_thread_first_nested_and_bare() {
        let code = "(-> x (f 1) (g 2))";
        let ast = parse(code.chars(), HashSet::new());
        let root = SyntaxNode::new_root(ast.root.clone());
        let info = expand_macro_at_offset(&root, 1).expect("expected macro info");
        assert_eq!(info.expansion, "(g (f x 1) 2)");

        let code_bare = "(-> x foo)";
        let ast_bare = parse(code_bare.chars(), HashSet::new());
        let root_bare = SyntaxNode::new_root(ast_bare.root.clone());
        let info_bare = expand_macro_at_offset(&root_bare, 1).expect("expected macro info");
        assert_eq!(info_bare.expansion, "(foo x)");
    }

    #[test]
    fn test_thread_last_expansion() {
        let code = "(->> x (f 1) (g 2))";
        let ast = parse(code.chars(), HashSet::new());
        let root = SyntaxNode::new_root(ast.root.clone());
        let info = expand_macro_at_offset(&root, 1).expect("expected macro info");
        assert_eq!(info.expansion, "(g 2 (f 1 x))");
        assert_eq!(info.signature, "(->> val ...)");
    }

    #[test]
    fn test_doto_expansion() {
        let code = "(doto x (foo 1) (bar 2))";
        let ast = parse(code.chars(), HashSet::new());
        let root = SyntaxNode::new_root(ast.root.clone());
        let info = expand_macro_at_offset(&root, 1).expect("expected macro info");
        assert_eq!(info.expansion, "(let [_val x] (foo _val 1) (bar _val 2) _val)");
    }

    #[test]
    fn test_when_expansion() {
        let code = "(when (> x 0) (print x) (foo x))";
        let ast = parse(code.chars(), HashSet::new());
        let root = SyntaxNode::new_root(ast.root.clone());
        let info = expand_macro_at_offset(&root, 1).expect("expected macro info");
        assert_eq!(info.expansion, "(if (> x 0) (do (print x) (foo x)))");
    }

    #[test]
    fn test_icollect_expansion() {
        let code = "(icollect [_ v (ipairs tbl)] (print v))";
        let ast = parse(code.chars(), HashSet::new());
        let root = SyntaxNode::new_root(ast.root.clone());
        let info = expand_macro_at_offset(&root, 1).expect("expected macro info");
        assert_eq!(
            info.expansion,
            "(let [tbl_ []] (each [_ v (ipairs tbl)] (tset tbl_ (+ (length tbl_) 1) (print v))) tbl_)"
        );
    }
}
