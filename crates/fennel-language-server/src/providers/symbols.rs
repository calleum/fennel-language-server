//! Document/workspace symbols, selection and folding ranges, highlights.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::collections::HashSet;

use fennel_parser::{SyntaxKind, SyntaxNode};
use ropey::Rope;
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{
    Backend,
    helper::*,
    view::{document_symbols_view, value_kind_to_symbol_kind},
};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;

        let symbols = ast.document_symbols();

        fn to_lsp_symbol(
            doc: &Rope,
            sym: &fennel_parser::AstDocumentSymbol,
        ) -> Option<DocumentSymbol> {
            let range = lsp_range(doc, sym.range).ok()?;
            let selection_range = lsp_range(doc, sym.selection_range).ok()?;

            let children = sym.children.as_ref().map(|children| {
                children
                    .iter()
                    .filter(|child| document_symbols_view(&child.kind))
                    .filter_map(|child| to_lsp_symbol(doc, child))
                    .collect()
            });

            #[expect(deprecated, reason = "deprecated is deprecated")]
            Some(DocumentSymbol {
                name: sym.name.clone(),
                detail: sym.detail.clone(),
                kind: value_kind_to_symbol_kind(sym.kind.clone()),
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children,
            })
        }

        let lsp_symbols: Vec<DocumentSymbol> =
            symbols.iter().filter_map(|s| to_lsp_symbol(&doc, s)).collect();
        Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
    }

    pub(crate) async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>, Error> {
        let query = params.query.to_lowercase();
        let mut result = Vec::new();

        for entry in self.ast_map.iter() {
            let uri = entry.key();
            let ast = entry.value();

            let Some(doc) = self.doc_map.get(uri) else {
                continue;
            };

            for sym in ast.workspace_symbols() {
                if !document_symbols_view(&sym.kind) {
                    continue;
                }
                if !query.is_empty() && !sym.name.to_lowercase().contains(&query) {
                    continue;
                }

                if let Ok(range) = lsp_range(&doc, sym.location) {
                    #[expect(deprecated, reason = "deprecated is deprecated in SymbolInformation")]
                    result.push(SymbolInformation {
                        name: sym.name,
                        kind: value_kind_to_symbol_kind(sym.kind),
                        tags: None,
                        deprecated: None,
                        location: Location::new(uri.clone(), range),
                        container_name: sym.container_name,
                    });
                }
            }
        }

        Ok(Some(result))
    }

    pub(crate) async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let root = SyntaxNode::new_root(ast.root.clone());

        let mut results = Vec::new();
        for position in params.positions {
            let offset = position_to_byte_offset(&doc, position)?;
            let text_size = rowan::TextSize::from(offset);

            let token_opt = match root.token_at_offset(text_size) {
                rowan::TokenAtOffset::Single(token) => Some(token),
                rowan::TokenAtOffset::Between(left, right) => {
                    if right.kind() != SyntaxKind::WHITESPACE { Some(right) } else { Some(left) }
                }
                rowan::TokenAtOffset::None => None,
            };

            let Some(token) = token_opt else {
                continue;
            };

            let mut ranges = Vec::new();
            if token.kind() != SyntaxKind::WHITESPACE {
                ranges.push(token.text_range());
            }

            for ancestor in token.parent().into_iter().flat_map(|p| p.ancestors()) {
                if is_selection_node_kind(ancestor.kind()) {
                    let range = ancestor.text_range();
                    if ranges.last() != Some(&range) {
                        ranges.push(range);
                    }
                }
            }

            let mut current: Option<SelectionRange> = None;
            for r in ranges.into_iter().rev() {
                if let Ok(lsp_range) = lsp_range(&doc, r) {
                    current =
                        Some(SelectionRange { range: lsp_range, parent: current.map(Box::new) });
                }
            }

            if let Some(sr) = current {
                results.push(sr);
            }
        }

        Ok(Some(results))
    }

    pub(crate) async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let root = SyntaxNode::new_root(ast.root.clone());

        let mut folds = Vec::new();
        let mut seen = HashSet::new();

        for node in root.descendants() {
            if is_folding_node_kind(node.kind()) {
                let range = node.text_range();
                if let (Ok(start_pos), Ok(end_pos)) = (
                    byte_offset_to_position(&doc, range.start().into()),
                    byte_offset_to_position(&doc, range.end().into()),
                ) {
                    let end_line = if end_pos.character == 0 && end_pos.line > start_pos.line {
                        end_pos.line - 1
                    } else {
                        end_pos.line
                    };
                    if end_line > start_pos.line && seen.insert((start_pos.line, end_line)) {
                        folds.push(FoldingRange {
                            start_line: start_pos.line,
                            start_character: None,
                            end_line,
                            end_character: None,
                            kind: None,
                            collapsed_text: None,
                        });
                    }
                }
            }
        }

        let mut current_comment_start: Option<u32> = None;
        let mut current_comment_end: Option<u32> = None;

        for element in root.descendants_with_tokens() {
            let Some(token) = element.into_token() else {
                continue;
            };
            if token.kind() == SyntaxKind::COMMENT {
                let range = token.text_range();
                if let (Ok(start_pos), Ok(end_pos)) = (
                    byte_offset_to_position(&doc, range.start().into()),
                    byte_offset_to_position(&doc, range.end().into()),
                ) {
                    let end_line = if end_pos.character == 0 && end_pos.line > start_pos.line {
                        end_pos.line - 1
                    } else {
                        end_pos.line
                    };
                    if let (Some(prev_start), Some(prev_end)) =
                        (current_comment_start, current_comment_end)
                    {
                        if start_pos.line <= prev_end + 1 {
                            current_comment_end = Some(end_line.max(prev_end));
                        } else {
                            if prev_end > prev_start && seen.insert((prev_start, prev_end)) {
                                folds.push(FoldingRange {
                                    start_line: prev_start,
                                    start_character: None,
                                    end_line: prev_end,
                                    end_character: None,
                                    kind: Some(FoldingRangeKind::Comment),
                                    collapsed_text: None,
                                });
                            }
                            current_comment_start = Some(start_pos.line);
                            current_comment_end = Some(end_line);
                        }
                    } else {
                        current_comment_start = Some(start_pos.line);
                        current_comment_end = Some(end_line);
                    }
                }
            }
        }

        if let (Some(prev_start), Some(prev_end)) = (current_comment_start, current_comment_end)
            && prev_end > prev_start
            && seen.insert((prev_start, prev_end))
        {
            folds.push(FoldingRange {
                start_line: prev_start,
                start_character: None,
                end_line: prev_end,
                end_character: None,
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            });
        }

        Ok(Some(folds))
    }

    pub(crate) async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>, Error> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        let Some((def_range, usage_ranges)) = ast.document_highlights(offset) else {
            return Ok(None);
        };

        let mut highlights = Vec::new();
        if let Ok(range) = lsp_range(&doc, def_range) {
            highlights.push(DocumentHighlight { range, kind: Some(DocumentHighlightKind::WRITE) });
        }
        for usage in usage_ranges {
            if let Ok(range) = lsp_range(&doc, usage) {
                highlights
                    .push(DocumentHighlight { range, kind: Some(DocumentHighlightKind::READ) });
            }
        }

        Ok(Some(highlights))
    }
}

use super::{is_folding_node_kind, is_selection_node_kind};
