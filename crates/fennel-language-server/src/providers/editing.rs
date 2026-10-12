//! Code actions, commands, and inlay hints.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::collections::{HashMap, HashSet};

use fennel_parser::{SyntaxKind, SyntaxNode};
use ropey::Rope;
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, formatter, helper::*, view};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, params.range.start)?;
        let end_offset = position_to_byte_offset(&doc, params.range.end).unwrap_or(offset);

        let mut res: Vec<CodeActionOrCommand> = Vec::new();

        let actions = ast.hint_action(offset);
        for (range, action) in actions {
            let Some(range) = lsp_range(&doc, range).ok() else {
                continue;
            };
            let action = match action {
                fennel_parser::Action::ConvertToColonString(s) => {
                    let mut map = HashMap::new();
                    map.insert(uri.clone(), vec![TextEdit::new(range, s.to_owned())]);
                    CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Convert string to start with a colon".to_string(),
                        kind: Some(CodeActionKind::REFACTOR),
                        edit: Some(WorkspaceEdit::new(map)),
                        ..Default::default()
                    })
                }
                fennel_parser::Action::ConvertToQuoteString(s) => {
                    let mut map = HashMap::new();
                    map.insert(uri.clone(), vec![TextEdit::new(range, s.to_owned())]);
                    CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Convert string to double-quotes form".to_string(),
                        kind: Some(CodeActionKind::REFACTOR),
                        edit: Some(WorkspaceEdit::new(map)),
                        ..Default::default()
                    })
                }
            };
            res.push(action);
        }

        let mut seen = HashSet::new();
        let errors: Vec<fennel_parser::Error> = {
            let mut errs: Vec<fennel_parser::Error> = ast.errors().cloned().collect();
            if let Some(on_save) = self.on_save_or_open_errors.get(&uri) {
                errs.extend(on_save.iter().cloned());
            } else {
                errs.extend(ast.on_save_errors().cloned());
            }
            errs
        };

        for error in errors {
            let Ok(err_lsp_range) = lsp_range(&doc, error.range) else {
                continue;
            };

            let in_range = u32::from(error.range.start()) <= end_offset
                && offset <= u32::from(error.range.end());
            let diag_match =
                params.context.diagnostics.iter().any(|d| {
                    !(d.range.end < err_lsp_range.start || d.range.start > err_lsp_range.end)
                });

            if !in_range && !diag_match {
                continue;
            }

            match &error.kind {
                fennel_parser::ErrorKind::Unused => {
                    let Ok(start_char) = doc.try_byte_to_char(usize::from(error.range.start()))
                    else {
                        continue;
                    };
                    let Ok(end_char) = doc.try_byte_to_char(usize::from(error.range.end())) else {
                        continue;
                    };
                    let var_name = doc.slice(start_char..end_char).to_string();
                    if !var_name.starts_with('_')
                        && !var_name.is_empty()
                        && seen.insert((error.range, "unused".to_string()))
                    {
                        let mut map = HashMap::new();
                        map.insert(
                            uri.clone(),
                            vec![TextEdit::new(err_lsp_range, format!("_{var_name}"))],
                        );

                        let matching_diags: Vec<Diagnostic> = params
                            .context
                            .diagnostics
                            .iter()
                            .filter(|d| {
                                !(d.range.end < err_lsp_range.start
                                    || d.range.start > err_lsp_range.end)
                            })
                            .cloned()
                            .collect();

                        res.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Prefix with _".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: if matching_diags.is_empty() {
                                let (msg, sev) = view::error(error.kind.clone());
                                Some(vec![Diagnostic::new(
                                    err_lsp_range,
                                    Some(sev),
                                    None,
                                    Some("Fennel Diagnostics".into()),
                                    msg,
                                    None,
                                    None,
                                )])
                            } else {
                                Some(matching_diags)
                            },
                            is_preferred: Some(true),
                            edit: Some(WorkspaceEdit::new(map)),
                            ..Default::default()
                        }));
                    }
                }
                fennel_parser::ErrorKind::Undefined(iden) => {
                    let base_symbol = iden.split('.').next().unwrap_or(iden);
                    if !base_symbol.is_empty()
                        && seen.insert((error.range, format!("undef:{base_symbol}")))
                    {
                        let matching_diags: Vec<Diagnostic> = params
                            .context
                            .diagnostics
                            .iter()
                            .filter(|d| {
                                !(d.range.end < err_lsp_range.start
                                    || d.range.start > err_lsp_range.end)
                            })
                            .cloned()
                            .collect();

                        res.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Add to globals configuration".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: if matching_diags.is_empty() {
                                let (msg, sev) = view::error(error.kind.clone());
                                Some(vec![Diagnostic::new(
                                    err_lsp_range,
                                    Some(sev),
                                    None,
                                    Some("Fennel Diagnostics".into()),
                                    msg,
                                    None,
                                    None,
                                )])
                            } else {
                                Some(matching_diags)
                            },
                            is_preferred: Some(true),
                            command: Some(Command {
                                title: "Add to globals configuration".to_string(),
                                command: "fennel.addGlobal".to_string(),
                                arguments: Some(vec![serde_json::Value::String(
                                    base_symbol.to_string(),
                                )]),
                            }),
                            ..Default::default()
                        }));
                    }
                }
                _ => {}
            }
        }

        if let Some(macro_info) = ast.expand_macro(offset)
            && let Ok(call_lsp_range) = lsp_range(&doc, macro_info.call_range)
        {
            let mut map = HashMap::new();
            map.insert(
                uri.clone(),
                vec![TextEdit::new(call_lsp_range, macro_info.expansion.clone())],
            );
            res.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Expand macro".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(WorkspaceEdit::new(map)),
                ..Default::default()
            }));
        }

        if let Some(only) = &params.context.only {
            res.retain(|action_or_cmd| {
                if let CodeActionOrCommand::CodeAction(ca) = action_or_cmd {
                    if let Some(kind) = &ca.kind {
                        only.iter().any(|allowed| kind.as_str().starts_with(allowed.as_str()))
                    } else {
                        false
                    }
                } else {
                    true
                }
            });
        }

        Ok(Some(res))
    }

    pub(crate) async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>, Error> {
        if params.command == "fennel.expandMacro" || params.command == "expandMacro" {
            let Some(uri_val) = params.arguments.first() else {
                return Ok(None);
            };
            let Some(uri_str) = uri_val.as_str() else {
                return Ok(None);
            };
            let uri = Url::parse(uri_str).map_err(|e| Error::invalid_params(e.to_string()))?;
            let Some(ast) = self.ast_map.get(&uri) else {
                return Ok(None);
            };
            let Some(doc) = self.doc_map.get(&uri) else {
                return Ok(None);
            };

            let offset = if let Some(pos_val) = params.arguments.get(1) {
                if let Ok(pos) = serde_json::from_value::<Position>(pos_val.clone()) {
                    position_to_byte_offset(&doc, pos)?
                } else if let Some(offset_num) = pos_val.as_u64() {
                    offset_num as u32
                } else {
                    0
                }
            } else {
                0
            };

            if let Some(macro_info) = ast.expand_macro(offset) {
                return Ok(Some(serde_json::Value::String(macro_info.expansion)));
            }
            return Ok(None);
        }

        if params.command == "fennel.addGlobal" || params.command == "addGlobal" {
            let Some(arg) = params.arguments.first() else {
                return Ok(None);
            };
            let Some(global_name) = arg.as_str() else {
                return Ok(None);
            };

            {
                let mut config = self.config.write().expect("config lock should not be poisoned");
                if !config.fennel.diagnostics.globals.iter().any(|g| g == global_name) {
                    config.fennel.diagnostics.globals.push(global_name.to_string());
                }
            }

            let globals = self.collect_globals();
            let updated: Vec<Url> = {
                let mut updated = Vec::new();
                for mut r in self.ast_map.iter_mut() {
                    let ast = r.value_mut();
                    ast.update_globals(globals.iter().cloned().collect());
                    updated.push(r.key().clone());
                }
                updated
            };

            for uri in updated {
                let Some(doc) = self.doc_map.get(&uri) else {
                    continue;
                };
                let Some(ast) = self.ast_map.get(&uri) else {
                    continue;
                };
                let diagnostics = self.compute_diagnostics(&doc, &uri, &ast, false);
                drop(doc);
                drop(ast);
                self.client.publish_diagnostics(uri, diagnostics, None).await;
            }

            return Ok(Some(serde_json::Value::Bool(true)));
        }
        Ok(None)
    }

    pub(crate) async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let root = SyntaxNode::new_root(ast.root.clone());

        let requested_range = params.range;
        let mut hints = Vec::new();

        for node in root.descendants() {
            if node.kind() == SyntaxKind::N_SYMBOL_CALL {
                let Some(callee_token) = node.first_token() else {
                    continue;
                };
                let callee_name = callee_token.text().to_string();
                let callee_offset = u32::from(callee_token.text_range().start());

                let Some((_label, params_list, _doc)) =
                    self.extract_func_signature(&callee_name, callee_offset, &ast)
                else {
                    continue;
                };

                let Some(args_node) = node.children().find(|n| n.kind() == SyntaxKind::N_ARGS)
                else {
                    continue;
                };

                let arg_nodes: Vec<_> =
                    args_node.children().filter(|n| n.kind() == SyntaxKind::N_SEXP).collect();

                for (i, arg) in arg_nodes.iter().enumerate() {
                    let Some(param_name) = params_list.get(i) else {
                        break;
                    };
                    if param_name == "&" || param_name == "..." || param_name.starts_with('&') {
                        break;
                    }

                    let arg_text = arg.text().to_string().trim().to_string();
                    if &arg_text == param_name {
                        continue;
                    }

                    let Ok(pos) = byte_offset_to_position(&doc, arg.text_range().start().into())
                    else {
                        continue;
                    };

                    if pos < requested_range.start || pos > requested_range.end {
                        continue;
                    }

                    hints.push(InlayHint {
                        position: pos,
                        label: InlayHintLabel::String(format!("{param_name}:")),
                        kind: Some(InlayHintKind::PARAMETER),
                        text_edits: None,
                        tooltip: None,
                        padding_left: None,
                        padding_right: Some(true),
                        data: None,
                    });
                }
            }
        }

        Ok(Some(hints))
    }

    /// Formats the whole document.
    ///
    /// Formatting is declined when the source does not parse cleanly: a
    /// formatter that rewrites code it does not understand can turn a typo
    /// into a different typo, and the user's buffer is the source of truth.
    pub(crate) async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>, Error> {
        let uri = params.text_document.uri;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let text = doc.to_string();

        let Some(formatted) = formatter::format(&text) else {
            return Ok(None);
        };

        if formatted == text {
            // No change: returning an empty edit avoids marking the buffer
            // dirty in the client.
            return Ok(Some(vec![]));
        }

        let range = Range::new(Position::new(0, 0), position_of_end(&doc));
        Ok(Some(vec![TextEdit::new(range, formatted)]))
    }

    pub(crate) async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>, Error> {
        let uri = params.text_document.uri;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;

        // Formatting a fragment in isolation is not meaningful here: the
        // formatter works on whole documents so it can re-indent relative to
        // the enclosing form. Decline explicitly rather than corrupting text.
        let _ = &doc;
        self.client
            .log_message(
                MessageType::INFO,
                "range formatting is not supported; format the whole document instead",
            )
            .await;
        Ok(None)
    }
}

/// End-of-document position, used for whole-document text edits.
fn position_of_end(doc: &Rope) -> Position {
    let line = doc.len_lines().saturating_sub(1);
    let line_start = doc.line_to_char(line);
    let character = doc.len_chars().saturating_sub(line_start);
    Position::new(line as u32, character as u32)
}
