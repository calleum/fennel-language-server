//! Rename support, including cross-file edits.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::collections::HashMap;

use fennel_parser::{SyntaxKind, SyntaxNode, models};
use rowan::{TextRange, TextSize};
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, helper::*};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>, Error> {
        let uri = params.text_document.uri;
        let position = params.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        let root = SyntaxNode::new_root(ast.root.clone());
        let token = match root.token_at_offset(TextSize::from(offset)).right_biased() {
            Some(t) => t,
            None => return Ok(None),
        };

        if token.kind() != SyntaxKind::SYMBOL
            && token.kind() != SyntaxKind::SYMBOL_FIELD
            && token.kind() != SyntaxKind::SYMBOL_METHOD
        {
            return Ok(None);
        }

        let token_start: u32 = token.text_range().start().into();
        let l_symbol = ast.l_symbol(token_start).cloned().or_else(|| {
            let r_symbol = ast.r_symbol(token_start).or_else(|| ast.r_symbol(offset))?;
            ast.resolve_l_symbol_by_r(r_symbol).cloned()
        });

        let is_renamable = l_symbol.is_some()
            || matches!(
                ast.definition(offset),
                Some(fennel_parser::Definition::Symbol(_, _))
                    | Some(fennel_parser::Definition::SymbolField(_, _))
                    | Some(fennel_parser::Definition::FileSymbol(_, _))
            );

        if !is_renamable {
            return Ok(None);
        }

        let (range, placeholder) = if token.kind() == SyntaxKind::SYMBOL_FIELD
            || token.kind() == SyntaxKind::SYMBOL_METHOD
        {
            let text = token.text();
            if text.starts_with('.') || text.starts_with(':') {
                let trimmed_range = TextRange::new(
                    token.text_range().start() + TextSize::from(1),
                    token.text_range().end(),
                );
                (lsp_range(&doc, trimmed_range)?, text.trim_start_matches(['.', ':']).to_string())
            } else {
                (lsp_range(&doc, token.text_range())?, text.to_string())
            }
        } else {
            (lsp_range(&doc, token.text_range())?, token.text().to_string())
        };

        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }))
    }

    pub(crate) async fn rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>, Error> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        if !ast.validate_name(&params.new_name) {
            return Err(Error::invalid_params("Illegal identifier name"));
        }

        let root = SyntaxNode::new_root(ast.root.clone());
        let token = match root.token_at_offset(TextSize::from(offset)).right_biased() {
            Some(t) => t,
            None => return Ok(None),
        };
        let token_start: u32 = token.text_range().start().into();

        let in_file_ranges = ast.reference(offset);

        let (target_def_uri, target_name) = match ast.definition(offset) {
            Some(fennel_parser::Definition::Symbol(lsym, _)) => {
                (uri.clone(), lsym.token.text.clone())
            }
            Some(fennel_parser::Definition::SymbolField(lsym, field)) => {
                let bare_name = field.split('.').next_back().unwrap_or(&field).to_string();
                if let models::ValueKind::Require(Some(file)) = &lsym.value.kind {
                    if let Some(def_uri) = self.find_file(&uri, file.clone()) {
                        (def_uri, bare_name)
                    } else {
                        (uri.clone(), bare_name)
                    }
                } else {
                    (uri.clone(), bare_name)
                }
            }
            Some(fennel_parser::Definition::FileSymbol(path, lsym)) => {
                if let Some(def_uri) = self.find_file(&uri, path) {
                    (def_uri, lsym.token.text.clone())
                } else {
                    (uri.clone(), lsym.token.text.clone())
                }
            }
            Some(fennel_parser::Definition::File(_)) => return Ok(None),
            None => {
                let l_sym = ast.l_symbol(token_start).cloned().or_else(|| {
                    let r_sym = ast.r_symbol(token_start).or_else(|| ast.r_symbol(offset))?;
                    ast.resolve_l_symbol_by_r(r_sym).cloned()
                });
                if let Some(lsym) = l_sym {
                    (uri.clone(), lsym.token.text.clone())
                } else {
                    return Ok(None);
                }
            }
        };

        drop(ast);
        drop(doc);

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        if uri == target_def_uri {
            if let Some(ranges) = in_file_ranges {
                let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
                let mut edits = Vec::with_capacity(ranges.len());
                for r in ranges {
                    if let Ok(range) = lsp_range(&doc, r) {
                        edits.push(TextEdit::new(range, params.new_name.clone()));
                    }
                }
                changes.insert(uri.clone(), edits);
            }
        } else if let Some(def_ast) = self.ast_map.get(&target_def_uri)
            && let Some(def_doc) = self.doc_map.get(&target_def_uri)
        {
            let lsym = def_ast.all_l_symbols().find(|s| s.token.text == target_name);
            if let Some(lsym) = lsym
                && let Some(ranges) = def_ast.reference(lsym.token.range.start().into())
            {
                let mut edits = Vec::new();
                for r in ranges {
                    if let Ok(range) = lsp_range(&def_doc, r) {
                        edits.push(TextEdit::new(range, params.new_name.clone()));
                    }
                }
                changes.insert(target_def_uri.clone(), edits);
            }
        }

        for r in self.ast_map.iter() {
            let other_uri = r.key().clone();
            if other_uri == target_def_uri {
                continue;
            }
            let other_ast = r.value();
            let Some(other_doc) = self.doc_map.get(&other_uri) else {
                continue;
            };

            let mut file_edits = Vec::new();

            for lsym in other_ast.all_l_symbols() {
                let is_alias = match &lsym.value.kind {
                    models::ValueKind::ModuleField(file, fields) => {
                        fields.last().is_some_and(|f| f == &target_name)
                            && self.find_file(&other_uri, file.clone())
                                == Some(target_def_uri.clone())
                    }
                    _ => {
                        if let Some(fennel_parser::Definition::FileSymbol(path, file_sym)) =
                            other_ast.definition(lsym.token.range.start().into())
                        {
                            file_sym.token.text == target_name
                                && self.find_file(&other_uri, path) == Some(target_def_uri.clone())
                        } else {
                            false
                        }
                    }
                };

                if is_alias && let Some(refs) = other_ast.reference(lsym.token.range.start().into())
                {
                    for range in refs {
                        if let Ok(range) = lsp_range(&other_doc, range) {
                            file_edits.push(TextEdit::new(range, params.new_name.clone()));
                        }
                    }
                }
            }

            for r_sym in other_ast.r_symbols() {
                if field_segment_offset(&r_sym.token.text, r_sym.token.range, &target_name)
                    .is_none()
                {
                    continue;
                }
                let base_name = match r_sym.token.text.split('.').next() {
                    Some(b) => b,
                    None => continue,
                };
                let base_token = fennel_parser::models::Token {
                    text: base_name.to_string(),
                    range: r_sym.token.range,
                };
                let base_rsym =
                    models::RSymbol { token: base_token, special: models::SpecialKind::Normal };
                let is_target_module = if let Some(blsym) =
                    other_ast.resolve_l_symbol_by_r(&base_rsym)
                {
                    match &blsym.value.kind {
                        models::ValueKind::Require(Some(file)) => {
                            self.find_file(&other_uri, file.clone()) == Some(target_def_uri.clone())
                        }
                        _ => false,
                    }
                } else {
                    false
                };

                if is_target_module {
                    // Locate the field segment in the symbol text rather
                    // than deriving it from byte lengths, so multi-byte
                    // identifiers and names that are a suffix of another
                    // segment cannot produce a misaligned edit.
                    let Some(field_start) =
                        field_segment_offset(&r_sym.token.text, r_sym.token.range, &target_name)
                    else {
                        continue;
                    };
                    let field_range = TextRange::new(field_start, r_sym.token.range.end());
                    if let Ok(range) = lsp_range(&other_doc, field_range) {
                        file_edits.push(TextEdit::new(range, params.new_name.clone()));
                    }
                }
            }

            if !file_edits.is_empty() {
                changes.entry(other_uri).or_default().extend(file_edits);
            }
        }

        for edits in changes.values_mut() {
            edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));
            edits.dedup_by_key(|e| e.range);
        }

        if changes.is_empty() { Ok(None) } else { Ok(Some(WorkspaceEdit::new(changes))) }
    }
}

use super::field_segment_offset;
