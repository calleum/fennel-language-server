//! Parser, schema validator, and configuration loader for `flsproject.fnl`.
//!
//! Provides behavioral and structural conformance to `fennel-ls`.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use rowan::{TextRange, ast::AstNode};
use serde::{Deserialize, Serialize};

use fennel_parser::{
    SyntaxKind, SyntaxNode,
    ast::{eval::EvalAst, nodes},
    models,
};

pub const VALID_TOP_LEVEL_KEYS: [&str; 8] = [
    "compiler-instruction-limit",
    "extra-globals",
    "fennel-path",
    "libraries",
    "lints",
    "lua-version",
    "macro-path",
    "plugins",
];

pub const KNOWN_LUA_VERSIONS: [&str; 12] = [
    "intersection",
    "lua5.1",
    "lua5.2",
    "lua5.3",
    "lua5.4",
    "lua5.5",
    "lua51",
    "lua52",
    "lua53",
    "lua54",
    "lua55",
    "union",
];

pub const VALID_LINT_KEYS: [&str; 23] = [
    "bad-unpack",
    "duplicate-table-keys",
    "empty-do",
    "empty-let",
    "inline-unpack",
    "invalid-flsproject-settings",
    "legacy-multival",
    "legacy-multival-case",
    "match-should-case",
    "nested-associative-operator",
    "no-decreasing-comparison",
    "not-enough-arguments",
    "op-with-no-arguments",
    "operator-values",
    "redundant-do",
    "too-many-arguments",
    "unknown-module-field",
    "unnecessary-method",
    "unnecessary-tset",
    "unnecessary-unary",
    "unused-definition",
    "var-never-set",
    "zero-indexed",
];

pub fn default_lints() -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for lint in VALID_LINT_KEYS {
        let disabled = matches!(
            lint,
            "legacy-multival"
                | "legacy-multival-case"
                | "no-decreasing-comparison"
                | "not-enough-arguments"
                | "zero-indexed"
        );
        map.insert(lint.to_string(), !disabled);
    }
    map
}

pub fn format_valid_keys(keys: &[&str]) -> String {
    let mut sorted = keys.to_vec();
    sorted.sort_unstable();
    let body = sorted.iter().map(|k| format!("\"{}\"", k)).collect::<Vec<_>>().join("\n ");
    format!("[{}]", body)
}

pub fn docset_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("fennel-ls/docsets");
    }
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/fennel-ls/docsets")
    } else {
        PathBuf::from(".local/share/fennel-ls/docsets")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct FlsProjectConfig {
    #[serde(alias = "fennelPath")]
    pub fennel_path: String,
    #[serde(alias = "macroPath")]
    pub macro_path: String,
    #[serde(alias = "luaVersion")]
    pub lua_version: String,
    #[serde(alias = "extraGlobals")]
    pub extra_globals: String,
    pub libraries: HashMap<String, bool>,
    pub lints: HashMap<String, bool>,
    pub plugins: Vec<String>,
    #[serde(alias = "compilerInstructionLimit")]
    pub compiler_instruction_limit: i64,
}

impl Default for FlsProjectConfig {
    fn default() -> Self {
        Self {
            fennel_path: "./?.fnl;./?/init.fnl;src/?.fnl;src/?/init.fnl".to_string(),
            macro_path: "./?.fnlm;./?/init.fnlm;./?.fnl;./?/init-macros.fnl;./?/init.fnl;src/?.fnlm;src/?/init.fnlm;src/?.fnl;src/?/init-macros.fnl;src/?/init.fnl".to_string(),
            lua_version: "lua54".to_string(),
            extra_globals: String::new(),
            libraries: HashMap::new(),
            lints: default_lints(),
            plugins: Vec::new(),
            compiler_instruction_limit: -1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlsValidationError {
    pub range: TextRange,
    pub message: String,
}

pub fn parse_and_validate_flsproject(text: &str) -> (FlsProjectConfig, Vec<FlsValidationError>) {
    let mut config = FlsProjectConfig::default();
    let mut errors = Vec::new();

    let ast = fennel_parser::parse(text.chars(), HashSet::new());
    for err in ast.errors() {
        errors.push(FlsValidationError {
            range: err.range,
            message: format!("syntax error: {}", err.kind),
        });
    }

    let root = SyntaxNode::new_root(ast.root);
    let sexps: Vec<SyntaxNode> =
        root.children().filter(|n| n.kind() == SyntaxKind::N_SEXP).collect();

    let Some(first_sexp) = sexps.first() else {
        return (config, errors);
    };

    let kv_table_node = first_sexp.descendants().find(|n| n.kind() == SyntaxKind::N_KV_TABLE);

    let Some(kv_table_node) = kv_table_node else {
        errors.push(FlsValidationError {
            range: first_sexp.text_range(),
            message: "expected flsproject.fnl to be a table".to_string(),
        });
        return (config, errors);
    };

    let Some(kv_table) = nodes::KvTable::cast(kv_table_node) else {
        errors.push(FlsValidationError {
            range: first_sexp.text_range(),
            message: "expected flsproject.fnl to be a table".to_string(),
        });
        return (config, errors);
    };

    validate_table_pairs(&kv_table, &mut config, &mut errors);

    (config, errors)
}

fn extract_key_string_and_range(key_node: &SyntaxNode) -> Option<(String, TextRange)> {
    let lit_node = key_node
        .descendants()
        .find(|n| n.kind() == SyntaxKind::N_LITERAL || n.kind() == SyntaxKind::COLON_STRING)?;

    if let Some(token) = lit_node.first_token() {
        let text = token.text();
        let range = token.text_range();
        if let Some(stripped) = text.strip_prefix(':') {
            return Some((stripped.to_string(), range));
        }
        if let Some(inner) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return Some((inner.to_string(), range));
        }
        return Some((text.to_string(), range));
    }
    None
}

fn validate_table_pairs(
    kv_table: &nodes::KvTable,
    config: &mut FlsProjectConfig,
    errors: &mut Vec<FlsValidationError>,
) {
    for pair in kv_table.syntax().children().filter(|n| n.kind() == SyntaxKind::N_KV_PAIR) {
        let key_node = pair.children().find(|n| n.kind() == SyntaxKind::N_KEY);
        let value_node = pair.children().find(|n| n.kind() == SyntaxKind::N_VALUE);

        let Some(key_node) = key_node else { continue };
        let Some((key_name, key_range)) = extract_key_string_and_range(&key_node) else {
            continue;
        };

        if !VALID_TOP_LEVEL_KEYS.contains(&key_name.as_str()) {
            errors.push(FlsValidationError {
                range: key_range,
                message: format!(
                    "didn't expect {}\nvalid keys: {}",
                    key_name,
                    format_valid_keys(&VALID_TOP_LEVEL_KEYS)
                ),
            });
            continue;
        }

        let Some(value_node) = value_node else { continue };
        let val_ast = value_node.first_child().and_then(EvalAst::cast);
        let Some(val_ast) = val_ast else { continue };
        let val_range = value_node.text_range();

        match key_name.as_str() {
            "compiler-instruction-limit" => {
                if val_ast.eval_kind() == models::ValueKind::Number {
                    let text = val_ast.syntax().text().to_string();
                    if let Ok(limit) = text.parse::<i64>() {
                        config.compiler_instruction_limit = limit;
                    }
                } else {
                    errors.push(FlsValidationError {
                        range: val_range,
                        message: "compiler-instruction-limit must be a number".to_string(),
                    });
                }
            }
            "fennel-path" | "macro-path" | "extra-globals" | "lua-version" => {
                if let Some((s, _)) = val_ast.cast_string() {
                    match key_name.as_str() {
                        "fennel-path" => config.fennel_path = s,
                        "macro-path" => config.macro_path = s,
                        "extra-globals" => config.extra_globals = s,
                        "lua-version" => {
                            if KNOWN_LUA_VERSIONS.contains(&s.as_str()) {
                                config.lua_version = s;
                            } else {
                                errors.push(FlsValidationError {
                                    range: val_range,
                                    message: format!(
                                        "fennel-ls doesn't know about lua version {}\nThe known versions are: {}",
                                        s,
                                        format_valid_keys(&KNOWN_LUA_VERSIONS)
                                    ),
                                });
                            }
                        }
                        _ => {}
                    }
                } else {
                    errors.push(FlsValidationError {
                        range: val_range,
                        message: format!("{} must be a string", key_name),
                    });
                }
            }
            "libraries" => {
                let Some(lib_table) = val_ast.cast_kv_table() else {
                    errors.push(FlsValidationError {
                        range: val_range,
                        message: "libraries must be a table".to_string(),
                    });
                    continue;
                };

                let doc_dir = docset_dir();
                for lib_pair in
                    lib_table.syntax().children().filter(|n| n.kind() == SyntaxKind::N_KV_PAIR)
                {
                    let lib_key_node = lib_pair.children().find(|n| n.kind() == SyntaxKind::N_KEY);
                    let Some(lib_key_node) = lib_key_node else { continue };
                    let Some((lib_name, lib_range)) = extract_key_string_and_range(&lib_key_node)
                    else {
                        continue;
                    };

                    let lib_val_node =
                        lib_pair.children().find(|n| n.kind() == SyntaxKind::N_VALUE);
                    let enabled = lib_val_node
                        .and_then(|v| v.first_child())
                        .and_then(EvalAst::cast)
                        .map(|ast| ast.syntax().text() == "true")
                        .unwrap_or(true);

                    let docset_path = doc_dir.join(format!("{}.lua", lib_name));
                    if !docset_path.exists() {
                        errors.push(FlsValidationError {
                            range: lib_range,
                            message: format!(
                                "Could not find docset for library {} at {}\nSee https://wiki.fennel-lang.org/LanguageServer",
                                lib_name,
                                docset_path.display()
                            ),
                        });
                    } else {
                        config.libraries.insert(lib_name, enabled);
                    }
                }
            }
            "plugins" => {
                let is_seq = val_ast.eval_kind() == models::ValueKind::SeqTable;
                let is_kv = val_ast.cast_kv_table().is_some();
                if is_seq || is_kv {
                    let mut plugins = Vec::new();
                    for child in val_ast.syntax().descendants() {
                        if child.kind() == SyntaxKind::N_LITERAL
                            && let Some(token) = child.first_token()
                            && token.kind() == SyntaxKind::QUOTE_STRING
                        {
                            let text = token.text();
                            if let Some(inner) =
                                text.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
                            {
                                plugins.push(inner.to_string());
                            }
                        }
                    }
                    config.plugins = plugins;
                } else {
                    errors.push(FlsValidationError {
                        range: val_range,
                        message: "plugins must be a table".to_string(),
                    });
                }
            }
            "lints" => {
                let Some(lints_table) = val_ast.cast_kv_table() else {
                    errors.push(FlsValidationError {
                        range: val_range,
                        message: "expected lints to be a table".to_string(),
                    });
                    continue;
                };

                for lint_pair in
                    lints_table.syntax().children().filter(|n| n.kind() == SyntaxKind::N_KV_PAIR)
                {
                    let lint_key_node =
                        lint_pair.children().find(|n| n.kind() == SyntaxKind::N_KEY);
                    let Some(lint_key_node) = lint_key_node else { continue };
                    let Some((lint_name, lint_range)) =
                        extract_key_string_and_range(&lint_key_node)
                    else {
                        continue;
                    };

                    if !VALID_LINT_KEYS.contains(&lint_name.as_str()) {
                        errors.push(FlsValidationError {
                            range: lint_range,
                            message: format!(
                                "didn't expect lints.{}\nvalid keys: {}",
                                lint_name,
                                format_valid_keys(&VALID_LINT_KEYS)
                            ),
                        });
                        continue;
                    }

                    let lint_val_node =
                        lint_pair.children().find(|n| n.kind() == SyntaxKind::N_VALUE);
                    let Some(lint_val_node) = lint_val_node else { continue };
                    let lint_val_ast = lint_val_node.first_child().and_then(EvalAst::cast);
                    let Some(lint_val_ast) = lint_val_ast else { continue };

                    if lint_val_ast.eval_kind() == models::ValueKind::Bool {
                        let is_enabled = lint_val_ast.syntax().text() == "true";
                        config.lints.insert(lint_name, is_enabled);
                    } else {
                        errors.push(FlsValidationError {
                            range: lint_val_node.text_range(),
                            message: format!("lints.{} must be a boolean", lint_name),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_flsproject() {
        let code = "{:fennel-path \"./?/?/?/?.fnl\"\n :extra-globals \"foo-100 bar\"\n :lints {:unused-definition false}}";
        let (config, errors) = parse_and_validate_flsproject(code);
        assert!(errors.is_empty(), "expected 0 errors, got: {:?}", errors);
        assert_eq!(config.fennel_path, "./?/?/?/?.fnl");
        assert_eq!(config.extra_globals, "foo-100 bar");
        assert_eq!(config.lints.get("unused-definition"), Some(&false));
    }

    #[test]
    fn test_unknown_top_level_key() {
        let code = "{:invalid-key 123}";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.starts_with(
                "didn't expect invalid-key\nvalid keys: [\"compiler-instruction-limit\""
            )
        );
    }

    #[test]
    fn test_unknown_lint() {
        let code = "{:lints {:all false}}";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.starts_with("didn't expect lints.all\nvalid keys: [\"bad-unpack\"")
        );
    }

    #[test]
    fn test_invalid_types() {
        let code = "{:extra-globals 123 :compiler-instruction-limit \"high\" :libraries \"my-lib\" :plugins 42 :lints \"off\"}";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 5);
        assert!(errors.iter().any(|e| e.message == "extra-globals must be a string"));
        assert!(errors.iter().any(|e| e.message == "compiler-instruction-limit must be a number"));
        assert!(errors.iter().any(|e| e.message == "libraries must be a table"));
        assert!(errors.iter().any(|e| e.message == "plugins must be a table"));
        assert!(errors.iter().any(|e| e.message == "expected lints to be a table"));
    }

    #[test]
    fn test_lint_value_not_bool() {
        let code = "{:lints {:unused-definition \"no\"}}";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "lints.unused-definition must be a boolean");
    }

    #[test]
    fn test_root_not_table() {
        let code = "\"a string instead of table\"";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "expected flsproject.fnl to be a table");
    }

    #[test]
    fn test_invalid_lua_version() {
        let code = "{:lua-version \"lua5.0\"}";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("doesn't know about lua version lua5.0"));
    }

    #[test]
    fn test_missing_docset_library() {
        let code = "{:libraries {:nasilemak true}}";
        let (_, errors) = parse_and_validate_flsproject(code);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Could not find docset for library nasilemak"));
        assert!(errors[0].message.contains("https://wiki.fennel-lang.org/LanguageServer"));
    }
}
