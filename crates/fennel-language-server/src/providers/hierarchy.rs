//! Call hierarchy providers.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::collections::HashMap;

use fennel_parser::{SyntaxKind, SyntaxNode};
use rowan::TextSize;
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, helper::*};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>, Error> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;
        let text_size = TextSize::from(offset);
        let root = SyntaxNode::new_root(ast.root.clone());

        let token_opt = match root.token_at_offset(text_size) {
            rowan::TokenAtOffset::Single(token) => Some(token),
            rowan::TokenAtOffset::Between(left, right) => {
                if right.kind() != SyntaxKind::WHITESPACE { Some(right) } else { Some(left) }
            }
            rowan::TokenAtOffset::None => None,
        };

        let Some(token) = token_opt else {
            return Ok(None);
        };

        let fn_node =
            token.parent().and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::N_FN));

        let resolved_fn = if let Some(r_sym) = ast.r_symbol(offset)
            && let Some(def_lsym) = ast.resolve_l_symbol_by_r(r_sym)
        {
            let def_offset = u32::from(def_lsym.token.range.start());
            if let Some(def_token) = root.token_at_offset(TextSize::from(def_offset)).right_biased()
            {
                def_token
                    .parent()
                    .and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::N_FN))
            } else {
                None
            }
        } else {
            None
        };

        let target_node = resolved_fn.or(fn_node);
        let Some(target_node) = target_node else {
            return Ok(None);
        };

        if let Some(item) = get_enclosing_fn_info(&target_node, &doc, &uri) {
            Ok(Some(vec![item]))
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>, Error> {
        let target_name = &params.item.name;
        let mut caller_map: HashMap<(Url, u32, u32), (CallHierarchyItem, Vec<Range>)> =
            HashMap::new();

        for entry in self.ast_map.iter() {
            let file_uri = entry.key();
            let file_ast = entry.value();
            let Some(file_doc) = self.doc_map.get(file_uri) else {
                continue;
            };
            let root = SyntaxNode::new_root(file_ast.root.clone());

            for node in root.descendants() {
                if node.kind() == SyntaxKind::N_SYMBOL_CALL {
                    let Some(callee_token) = node.first_token() else {
                        continue;
                    };
                    let callee_text = callee_token.text();
                    let is_target = callee_text == target_name
                        || callee_text.ends_with(&format!(".{target_name}"))
                        || callee_text.ends_with(&format!(":{target_name}"));

                    if !is_target {
                        continue;
                    }

                    let Ok(call_range) = lsp_range(&file_doc, callee_token.text_range()) else {
                        continue;
                    };

                    let caller_fn = node.ancestors().find(|n| n.kind() == SyntaxKind::N_FN);

                    let caller_item = if let Some(caller_fn) = caller_fn
                        && let Some(item) = get_enclosing_fn_info(&caller_fn, &file_doc, file_uri)
                    {
                        item
                    } else {
                        let file_range = Range::new(
                            Position::new(0, 0),
                            byte_offset_to_position(&file_doc, file_doc.len_bytes())
                                .unwrap_or(Position::new(0, 0)),
                        );
                        CallHierarchyItem {
                            name: file_uri
                                .path_segments()
                                .and_then(|mut s| s.next_back())
                                .unwrap_or("file")
                                .to_string(),
                            kind: SymbolKind::MODULE,
                            tags: None,
                            detail: Some("top-level".to_string()),
                            uri: file_uri.clone(),
                            range: file_range,
                            selection_range: file_range,
                            data: None,
                        }
                    };

                    let key = (
                        caller_item.uri.clone(),
                        caller_item.selection_range.start.line,
                        caller_item.selection_range.start.character,
                    );
                    caller_map
                        .entry(key)
                        .or_insert_with(|| (caller_item, Vec::new()))
                        .1
                        .push(call_range);
                }
            }
        }

        let incoming: Vec<CallHierarchyIncomingCall> = caller_map
            .into_values()
            .map(|(from, from_ranges)| CallHierarchyIncomingCall { from, from_ranges })
            .collect();

        Ok(Some(incoming))
    }

    pub(crate) async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>, Error> {
        let uri = params.item.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let root = SyntaxNode::new_root(ast.root.clone());

        let offset = position_to_byte_offset(&doc, params.item.selection_range.start)?;
        let Some(name_token) = root.token_at_offset(TextSize::from(offset)).right_biased() else {
            return Ok(None);
        };
        let Some(fn_node) =
            name_token.parent().and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::N_FN))
        else {
            return Ok(None);
        };

        let mut callee_map: HashMap<(String, Url), (CallHierarchyItem, Vec<Range>)> =
            HashMap::new();

        for node in fn_node.descendants() {
            if node.kind() == SyntaxKind::N_SYMBOL_CALL {
                let Some(callee_token) = node.first_token() else {
                    continue;
                };
                let callee_text = callee_token.text().to_string();
                let callee_offset = u32::from(callee_token.text_range().start());

                let Ok(call_range) = lsp_range(&doc, callee_token.text_range()) else {
                    continue;
                };

                let callee_item = if let Some(r_sym) = ast.r_symbol(callee_offset)
                    && let Some(def_lsym) = ast.resolve_l_symbol_by_r(r_sym)
                {
                    let def_offset = u32::from(def_lsym.token.range.start());
                    if let Some(def_token) =
                        root.token_at_offset(TextSize::from(def_offset)).right_biased()
                        && let Some(def_fn) = def_token
                            .parent()
                            .and_then(|p| p.ancestors().find(|n| n.kind() == SyntaxKind::N_FN))
                    {
                        get_enclosing_fn_info(&def_fn, &doc, &uri)
                    } else {
                        let lsp_r = lsp_range(&doc, def_lsym.token.range).unwrap_or(call_range);
                        Some(CallHierarchyItem {
                            name: def_lsym.token.text.clone(),
                            kind: SymbolKind::FUNCTION,
                            tags: None,
                            detail: None,
                            uri: uri.clone(),
                            range: lsp_r,
                            selection_range: lsp_r,
                            data: None,
                        })
                    }
                } else {
                    Some(CallHierarchyItem {
                        name: callee_text.clone(),
                        kind: SymbolKind::FUNCTION,
                        tags: None,
                        detail: None,
                        uri: uri.clone(),
                        range: call_range,
                        selection_range: call_range,
                        data: None,
                    })
                };

                if let Some(callee_item) = callee_item {
                    let key = (callee_item.name.clone(), callee_item.uri.clone());
                    callee_map
                        .entry(key)
                        .or_insert_with(|| (callee_item, Vec::new()))
                        .1
                        .push(call_range);
                }
            }
        }

        let outgoing: Vec<CallHierarchyOutgoingCall> = callee_map
            .into_values()
            .map(|(to, from_ranges)| CallHierarchyOutgoingCall { to, from_ranges })
            .collect();

        Ok(Some(outgoing))
    }
}

use super::get_enclosing_fn_info;
