//! Completion, signature help, and document links.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::collections::HashSet;

use fennel_parser::{SyntaxKind, SyntaxNode, models};
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, helper::*, view};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>, Error> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        let trigger = params.context.and_then(|ctx| ctx.trigger_character);
        let (symbols, globals, base_value, base_text) = ast.completion(offset, trigger);
        let symbols = symbols.map(|symbol| {
            let (detail, documentation) = self.symbol_completion_detail_and_doc(symbol, &ast);
            CompletionItem {
                label: symbol.token.text.clone(),
                insert_text: Some(symbol.token.text.clone()),
                kind: Some(view::completion_scope_kind(symbol.scope.kind)),
                detail,
                documentation,
                ..Default::default()
            }
        });

        let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
        let std_completions = std_symbols.map(|symbol| {
            let (detail, documentation) =
                self.symbol_completion_detail_and_doc(symbol, &self.std_ast);
            CompletionItem {
                label: symbol.token.text.clone(),
                insert_text: Some(symbol.token.text.clone()),
                kind: Some(view::completion_scope_kind(symbol.scope.kind)),
                detail,
                documentation,
                ..Default::default()
            }
        });

        let globals = globals.into_iter().flat_map(|(kind, vec)| {
            vec.into_iter().map(move |word| CompletionItem {
                label: word.to_owned(),
                insert_text: Some(word.to_owned()),
                kind: Some(view::completion_value_kind(kind)),
                detail: Some(word.to_owned()),
                ..Default::default()
            })
        });
        let mut completions: Vec<CompletionItem> =
            symbols.chain(std_completions).chain(globals).collect();

        if let Some(val) = &base_value {
            match &val.kind {
                fennel_parser::models::ValueKind::Require(Some(path)) => {
                    if let Some(resolved_uri) = self.find_file(&uri, path.clone())
                        && let Some(resolved_ast) = self.get_or_parse_ast(&resolved_uri).await
                        && let Some(keys) = resolved_ast.return_kv_keys()
                    {
                        for k in keys {
                            completions.push(CompletionItem {
                                label: k.clone(),
                                insert_text: Some(k.clone()),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(k.clone()),
                                ..Default::default()
                            });
                        }
                    }
                }
                fennel_parser::models::ValueKind::Module => {
                    let (std_symbols, _, _, _) =
                        self.std_ast.completion(self.std_ast.end_offset(), None);
                    let prefix = format!("{}.", base_text);
                    for sym in std_symbols {
                        if sym.token.text.starts_with(&prefix) {
                            let field = sym
                                .token
                                .text
                                .strip_prefix(&prefix)
                                .ok_or_else(Error::invalid_request)?;
                            if !field.contains('.') {
                                let (detail, documentation) =
                                    self.symbol_completion_detail_and_doc(sym, &self.std_ast);
                                completions.push(CompletionItem {
                                    label: field.to_string(),
                                    insert_text: Some(field.to_string()),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail,
                                    documentation,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if base_value.is_none() {
            let existing_labels: HashSet<String> =
                completions.iter().map(|c| c.label.clone()).collect();
            let mut seen_imports = HashSet::new();

            for entry in self.ast_map.iter() {
                let other_uri = entry.key();
                if other_uri == &uri || other_uri == &self.std_uri {
                    continue;
                }
                let Some(mod_name) = self.uri_to_module_name(other_uri) else {
                    continue;
                };

                let other_ast = entry.value();
                let mut exports = Vec::new();

                if let Some(keys) = other_ast.return_kv_keys() {
                    exports.extend(keys);
                }
                for sym in other_ast.all_l_symbols() {
                    if sym.scope.kind != models::ScopeKind::Local
                        && !sym.token.text.starts_with('_')
                        && !exports.contains(&sym.token.text)
                    {
                        exports.push(sym.token.text.clone());
                    }
                }

                for export in exports {
                    if !existing_labels.contains(&export)
                        && seen_imports.insert((export.clone(), mod_name.clone()))
                    {
                        completions.push(CompletionItem {
                            label: export.clone(),
                            insert_text: Some(export.clone()),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some(format!("(auto-import from :{mod_name})")),
                            documentation: Some(Documentation::String(format!(
                                "Auto-import: `(local {export} (require :{mod_name}))`"
                            ))),
                            additional_text_edits: Some(vec![TextEdit::new(
                                Range::new(Position::new(0, 0), Position::new(0, 0)),
                                format!("(local {export} (require :{mod_name}))\n"),
                            )]),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        Ok(Some(CompletionResponse::Array(completions)))
    }

    pub(crate) async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>, Error> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;
        let root = SyntaxNode::new_root(ast.root.clone());

        let text_size = rowan::TextSize::from(offset);
        let token = match root.token_at_offset(text_size) {
            rowan::TokenAtOffset::Single(t) => Some(t),
            rowan::TokenAtOffset::Between(l, r) => {
                if r.kind() == SyntaxKind::R_PAREN {
                    Some(l)
                } else if r.kind() != SyntaxKind::WHITESPACE {
                    Some(r)
                } else {
                    Some(l)
                }
            }
            rowan::TokenAtOffset::None => None,
        };

        let Some(token) = token else {
            return Ok(None);
        };

        let Some(call_node) = token
            .parent_ancestors()
            .find(|n| matches!(n.kind(), SyntaxKind::N_SYMBOL_CALL | SyntaxKind::N_LIST))
        else {
            return Ok(None);
        };

        let symbol_call_opt = if call_node.kind() == SyntaxKind::N_SYMBOL_CALL {
            Some(call_node)
        } else {
            call_node.descendants().find(|n| n.kind() == SyntaxKind::N_SYMBOL_CALL)
        };
        let Some(symbol_call) = symbol_call_opt else {
            return Ok(None);
        };

        let Some(callee_token) = symbol_call.first_token() else {
            return Ok(None);
        };
        let callee_name = callee_token.text().to_string();
        let callee_offset = u32::from(callee_token.text_range().start());

        let Some((label, params, docstring)) =
            self.extract_func_signature(&callee_name, callee_offset, &ast)
        else {
            return Ok(None);
        };

        let mut active_param_idx = 0;
        if let Some(args_node) = symbol_call.children().find(|n| n.kind() == SyntaxKind::N_ARGS) {
            let arg_nodes: Vec<_> =
                args_node.children().filter(|n| n.kind() == SyntaxKind::N_SEXP).collect();
            let mut found = false;
            for (i, arg) in arg_nodes.iter().enumerate() {
                if offset <= u32::from(arg.text_range().end()) {
                    active_param_idx = i as u32;
                    found = true;
                    break;
                }
            }
            if !found && !arg_nodes.is_empty() {
                active_param_idx = arg_nodes.len() as u32;
            }
        }

        let sig_info = SignatureInformation {
            label,
            documentation: docstring.map(Documentation::String),
            parameters: Some(
                params
                    .iter()
                    .map(|p| ParameterInformation {
                        label: ParameterLabel::Simple(p.clone()),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: Some(active_param_idx),
        };

        Ok(Some(SignatureHelp {
            signatures: vec![sig_info],
            active_signature: Some(0),
            active_parameter: Some(active_param_idx),
        }))
    }

    pub(crate) async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> Result<Option<Vec<DocumentLink>>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;

        let mut links = Vec::new();
        for (path, text_range) in ast.require_links() {
            let target = self.find_file(&uri, path);
            if let Ok(range) = lsp_range(&doc, text_range) {
                links.push(DocumentLink { range, target, tooltip: None, data: None });
            }
        }

        Ok(Some(links))
    }
}
