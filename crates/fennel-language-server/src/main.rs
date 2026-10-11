mod cli;
mod config;
mod helper;
mod semantic_tokens;
mod view;

#[cfg(test)]
mod tdd_helper;
#[cfg(test)]
mod tdd_tests;
#[cfg(test)]
pub(crate) mod test_harness;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use clap::Parser;
use dashmap::DashMap;
use fennel_parser::{Ast, SyntaxKind, SyntaxNode, models};
use helper::*;
use ropey::Rope;
use rowan::{TextRange, TextSize, ast::AstNode};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};
use tower_lsp::{
    jsonrpc::{Error, Result},
    lsp_types::*,
};
use walkdir::WalkDir;

use crate::view::{document_symbols_view, value_kind_to_symbol_kind};

pub const STUBS_URI: &str = "fennel://stubs/lua54.fnl";

#[derive(Debug, Clone)]
struct Backend {
    client: tower_lsp::Client,
    config: Arc<RwLock<config::Configuration>>,
    doc_map: DashMap<Url, Rope>,
    ast_map: DashMap<Url, Ast>,
    workspace_map: DashMap<Url, String>,
    // publish those after saving
    on_save_or_open_errors: DashMap<Url, Vec<fennel_parser::Error>>,
    std_ast: Ast,
    std_uri: Url,
}

#[tower_lsp::async_trait]
impl tower_lsp::LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(folders) = params.workspace_folders {
            folders.into_iter().for_each(|folder| {
                self.workspace_map.insert(folder.uri, folder.name);
            });
        }

        if let Some(settings) = params.initialization_options
            && let Ok(config) = serde_json::from_value::<config::Configuration>(settings)
        {
            *self.config.write().map_err(|_err| Error::internal_error())? = config;
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: env!("CARGO_PKG_NAME").into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: None,
                    }),
                    file_operations: None,
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".into(), ":".into()]),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), " ".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["fennel.addGlobal".into()],
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: semantic_tokens::semantic_tokens_legend(),
                            range: None,
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                ..Default::default()
            },
        })
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        match ast.definition(offset) {
            Some(fennel_parser::Definition::Symbol(symbol, _))
            | Some(fennel_parser::Definition::SymbolField(symbol, _)) => {
                let range = lsp_range(&doc, symbol.token.range)?;
                match symbol.value.kind {
                    models::ValueKind::Require(Some(file)) => {
                        let res = self.find_file(&uri, file.clone());
                        if let Some(fennel_parser::Definition::SymbolField(_, field_text)) =
                            ast.definition(offset)
                            && let Some(new_uri) = res
                            && let Some(new_ast) = self.get_or_parse_ast(&new_uri).await
                        {
                            let fields: Vec<&str> = field_text.split('.').skip(1).collect();
                            if let Some(target_lsymbol) =
                                self.find_symbol_in_ast(&new_ast, &fields).await
                            {
                                let new_doc = self
                                    .doc_map
                                    .get(&new_uri)
                                    .ok_or_else(Error::invalid_request)?;
                                let new_range = lsp_range(&new_doc, target_lsymbol.token.range)?;
                                return Ok(Some(GotoDefinitionResponse::Array(vec![
                                    Location::new(uri.clone(), range),
                                    Location::new(new_uri, new_range),
                                ])));
                            }
                        }

                        self.find_file(&uri, file).map_or_else(
                            || {
                                Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                                    uri.clone(),
                                    range,
                                ))))
                            },
                            |new_uri| {
                                Ok(Some(GotoDefinitionResponse::Array(vec![
                                    Location::new(uri.clone(), range),
                                    Location::new(new_uri, lsp_range_head()),
                                ])))
                            },
                        )
                    }
                    models::ValueKind::ModuleField(file, fields) => {
                        if let Some(new_uri) = self.find_file(&uri, file) {
                            if let Some(new_ast) = self.get_or_parse_ast(&new_uri).await {
                                let fields_str: Vec<&str> =
                                    fields.iter().map(|s| s.as_str()).collect();
                                if let Some(target_lsymbol) =
                                    self.find_symbol_in_ast(&new_ast, &fields_str).await
                                {
                                    let new_doc = self
                                        .doc_map
                                        .get(&new_uri)
                                        .ok_or_else(Error::invalid_request)?;
                                    let new_range =
                                        lsp_range(&new_doc, target_lsymbol.token.range)?;
                                    return Ok(Some(GotoDefinitionResponse::Scalar(
                                        Location::new(new_uri, new_range),
                                    )));
                                }
                            }
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                                new_uri,
                                lsp_range_head(),
                            ))));
                        }
                        Ok(Some(GotoDefinitionResponse::Scalar(Location::new(uri, range))))
                    }
                    _ => Ok(Some(GotoDefinitionResponse::Scalar(Location::new(uri, range)))),
                }
            }
            Some(fennel_parser::Definition::FileSymbol(path, symbol)) => {
                let range = lsp_range(&doc, symbol.token.range)?;
                let res = self
                    .find_file(&uri, path)
                    .map(|uri| GotoDefinitionResponse::Scalar(Location::new(uri, range)));
                Ok(res)
            }
            Some(fennel_parser::Definition::File(path)) => {
                let res = self.find_file(&uri, path).map(|uri| {
                    GotoDefinitionResponse::Scalar(Location::new(uri, lsp_range_head()))
                });
                Ok(res)
            }
            None => {
                let r_symbol = ast
                    .r_symbol(offset)
                    .or_else(|| {
                        // try at offset - 1 for things like (print|)
                        if offset > 0 { ast.r_symbol(offset - 1) } else { None }
                    })
                    .ok_or_else(Error::invalid_request)?;
                let text = r_symbol.token.text.as_str();
                let fields: Vec<&str> = text.split('.').collect();
                let Some(base) = fields.first() else {
                    return Ok(None);
                };

                let std_doc = self.doc_map.get(&self.std_uri);
                let std_range_fn = |range: fennel_parser::TextRange| -> Range {
                    if let Some(doc) = &std_doc {
                        lsp_range(doc, range).unwrap_or_else(|_| lsp_range_head())
                    } else {
                        lsp_range_head()
                    }
                };

                let (std_symbols, _, _, _) =
                    self.std_ast.completion(self.std_ast.end_offset(), None);
                if let Some(target_lsymbol) = std_symbols.into_iter().find(|s| s.token.text == text)
                {
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                        self.std_uri.clone(),
                        std_range_fn(target_lsymbol.token.range),
                    ))));
                }

                if self.std_ast.definition_for_global(base).is_some() {
                    if fields.len() > 1 {
                        if let Some(target_lsymbol) = self
                            .find_symbol_in_ast(
                                &self.std_ast,
                                fields.get(1..).ok_or_else(Error::invalid_request)?,
                            )
                            .await
                        {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                                self.std_uri.clone(),
                                std_range_fn(target_lsymbol.token.range),
                            ))));
                        }
                    } else if let Some(fennel_parser::Definition::Symbol(lsym, _)) =
                        self.std_ast.definition_for_global(base)
                    {
                        return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                            self.std_uri.clone(),
                            std_range_fn(lsym.token.range),
                        ))));
                    }
                }
                Ok(None)
            }
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
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

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
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

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        // Get in-file references — handles both definition and usage offsets
        let Some(in_file_refs) = ast.reference(offset) else {
            return Ok(None);
        };
        if in_file_refs.is_empty() {
            return Err(Error::request_cancelled());
        }

        // Get the symbol name via definition() (works on both def and usage sites)
        let symbol_name = match ast.definition(offset) {
            Some(fennel_parser::Definition::Symbol(lsym, _)) => lsym.token.text.clone(),
            Some(fennel_parser::Definition::SymbolField(lsym, field)) => {
                // For field accesses like "foo.bar", search by the base
                if let Some(base) = field.split('.').next() {
                    format!("{}.{}", lsym.token.text, base)
                } else {
                    lsym.token.text.clone()
                }
            }
            _ => {
                return Ok(Some(
                    in_file_refs
                        .iter()
                        .filter_map(|r| lsp_range(&doc, *r).ok())
                        .map(|range| Location::new(uri.clone(), range))
                        .collect(),
                ));
            }
        };

        // Drop DashMap guards before iterating the map mutably (avoids deadlock)
        drop(ast);
        drop(doc);

        let fields: Vec<&str> = symbol_name.split('.').collect();

        // Search other files for the same symbol
        let mut cross_file_refs: HashMap<Url, Vec<models::RSymbol>> = HashMap::new();
        for r in self.ast_map.iter() {
            let ast = r.value();
            if let Some(sym) = self.find_r_symbol_in_ast(ast, &fields).await {
                cross_file_refs.entry(r.key().clone()).or_default().push(sym);
            }
        }

        let mut locations: Vec<Location> = Vec::new();

        // Cross-file references
        for (ref_uri, symbols) in &cross_file_refs {
            let ref_doc = match self.doc_map.get(ref_uri) {
                Some(d) => d,
                None => continue,
            };
            for sym in symbols {
                if let Ok(range) = lsp_range(&ref_doc, sym.token.range) {
                    locations.push(Location::new(ref_uri.clone(), range));
                }
            }
        }

        // In-file references (re-acquire guards)
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        for reference in in_file_refs {
            let range = lsp_range(&doc, reference)?;
            locations.push(Location::new(uri.clone(), range));
        }
        Ok(Some(locations))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
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

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
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
                if r_sym.token.text.ends_with(&format!(".{}", target_name)) {
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
                    let is_target_module =
                        if let Some(blsym) = other_ast.resolve_l_symbol_by_r(&base_rsym) {
                            match &blsym.value.kind {
                                models::ValueKind::Require(Some(file)) => {
                                    self.find_file(&other_uri, file.clone())
                                        == Some(target_def_uri.clone())
                                }
                                _ => false,
                            }
                        } else {
                            false
                        };

                    if is_target_module {
                        let target_len = TextSize::from(target_name.len() as u32);
                        let start_offset = if r_sym.token.range.len() > target_len {
                            r_sym.token.range.start() + (r_sym.token.range.len() - target_len)
                        } else {
                            r_sym.token.range.start()
                        };
                        let field_range = TextRange::new(start_offset, r_sym.token.range.end());
                        if let Ok(range) = lsp_range(&other_doc, field_range) {
                            file_edits.push(TextEdit::new(range, params.new_name.clone()));
                        }
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

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
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

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
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

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        let (symbol, field_text) = match ast.definition(offset) {
            Some(fennel_parser::Definition::Symbol(symbol, _)) => (symbol, None),
            Some(fennel_parser::Definition::SymbolField(symbol, field)) => (symbol, Some(field)),
            _ => {
                let root = SyntaxNode::new_root(ast.root.clone());
                if let Some(token) = root.token_at_offset(TextSize::from(offset)).right_biased()
                    && let Some(lua_node) =
                        token.parent_ancestors().find_map(fennel_parser::ast::nodes::Lua::cast)
                    && let Some(code) = lua_node.code()
                {
                    let range = lsp_range(&doc, lua_node.syntax().text_range())?;
                    let contents =
                        HoverContents::Scalar(MarkedString::LanguageString(LanguageString {
                            language: "lua".into(),
                            value: code,
                        }));
                    return Ok(Some(Hover { contents, range: Some(range) }));
                }
                return Ok(None);
            }
        };
        let range = lsp_range(&doc, symbol.token.range)?;
        let text = field_text.unwrap_or(symbol.token.text);
        let scope_kind = view::scope_kind(symbol.scope.kind);
        let value_kind = view::value_kind(&symbol.value.kind);

        let header_text = format!(
            "{} {}{}{}",
            scope_kind,
            text,
            if value_kind.is_empty() { "".to_owned() } else { " : ".to_owned() + value_kind },
            if let Some(literal) = ast.literal_value(symbol.value) {
                let prefix = if literal.contains('\n') { " =\n" } else { " = " };
                prefix.to_owned() + &literal
            } else {
                "".to_owned()
            },
        );
        let body_text = if symbol.scope.kind == models::ScopeKind::Func {
            ast.docstring(symbol.token.range)
        } else {
            None
        };

        let header = MarkedString::LanguageString(LanguageString {
            language: "fennel".into(),
            value: header_text,
        });
        let contents = if let Some(body_text) = body_text {
            HoverContents::Array(vec![
                header,
                MarkedString::LanguageString(LanguageString {
                    language: "markdown".into(),
                    value: body_text,
                }),
            ])
        } else {
            HoverContents::Scalar(header)
        };
        Ok(Some(Hover { contents, range: Some(range) }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
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

        if let Some(val) = base_value {
            match val.kind {
                fennel_parser::models::ValueKind::Require(Some(path)) => {
                    if let Some(resolved_uri) = self.find_file(&uri, path)
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

        Ok(Some(CompletionResponse::Array(completions)))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
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

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
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
            for mut r in self.ast_map.iter_mut() {
                let ast = r.value_mut();
                ast.update_globals(globals.iter().cloned().collect());
                let uri = r.key();
                if let Some(doc) = self.doc_map.get(uri) {
                    self.publish_diagnostics(&doc, uri.clone(), r.value(), None, false).await;
                }
            }

            return Ok(Some(serde_json::Value::Bool(true)));
        }
        Ok(None)
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
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

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
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

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
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

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;

        let tokens = semantic_tokens::semantic_tokens_full(&doc, &ast, &self.std_ast);
        Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
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

    async fn initialized(&self, _: InitializedParams) {
        self.client.log_message(MessageType::LOG, "initialized!").await;
        for fol in self.workspace_map.iter() {
            if let Ok(files) = collect_fnl_filepaths(fol.key().clone()) {
                for f in &files {
                    self.get_or_parse_ast(f).await;
                }
            }
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client.log_message(MessageType::LOG, "file opened!").await;
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        let doc = ropey::Rope::from_str(&text);
        self.doc_map.insert(uri.clone(), doc.clone());

        let globals = self.collect_globals();
        let ast = fennel_parser::parse(text.chars(), globals);
        self.publish_diagnostics(&doc, uri.clone(), &ast, Some(version), true).await;

        self.on_save_or_open_errors.insert(uri.clone(), ast.on_save_errors().cloned().collect());

        self.ast_map.insert(uri, ast);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let mut doc = if let Some(doc) = self.doc_map.get_mut(&uri) {
            doc
        } else {
            return;
        };

        params.content_changes.iter().for_each(|change| {
            if let Some(lsp_range) = change.range {
                let range =
                    rope_range(&doc, lsp_range).expect("LSP range should be valid for document");
                doc.remove(range.clone());
                if !change.text.is_empty() {
                    doc.insert(range.start, &change.text);
                }
            } else {
                *doc = Rope::from_str(&change.text);
            }
        });

        let globals = self.collect_globals();
        let ast = fennel_parser::parse(doc.chars(), globals);
        self.publish_diagnostics(&doc, uri.clone(), &ast, Some(version), false).await;

        self.ast_map.insert(uri, ast);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(ast) = self.ast_map.get(&uri) else {
            return;
        };
        let Some(doc) = self.doc_map.get(&uri) else {
            return;
        };
        self.publish_diagnostics(&doc, uri, &ast, None, true).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.free_doc(&uri);
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        params.event.added.iter().for_each(|r| {
            self.workspace_map.insert(r.uri.clone(), r.name.clone());
        });
        params.event.removed.iter().for_each(|r| {
            self.workspace_map.remove(&r.uri);
        });
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        match <config::Configuration as serde::Deserialize>::deserialize(params.settings) {
            Ok(config) => {
                *self.config.write().expect("config lock should not be poisoned") = config.clone();

                let globals = self.collect_globals();

                for mut r in self.ast_map.iter_mut() {
                    let ast = r.value_mut();
                    ast.update_globals(globals.iter().cloned().collect());
                    let uri = r.key();
                    let doc = self
                        .doc_map
                        .get(uri)
                        .expect("document should exist in doc_map if it exists in ast_map");
                    self.publish_diagnostics(&doc, uri.clone(), r.value(), None, false).await;
                }
            }
            Err(e) => {
                self.client.log_message(MessageType::ERROR, format!("Invalid config: {}", e)).await;
            }
        }
    }
}

impl Backend {
    pub(crate) fn collect_globals(&self) -> HashSet<String> {
        let mut globals = HashSet::new();
        if let Ok(config) = self.config.read() {
            for global in &config.fennel.diagnostics.globals {
                globals.insert(global.clone());
            }
        }
        let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
        for sym in std_symbols {
            globals.insert(sym.token.text.clone());
        }
        globals
    }

    async fn get_or_parse_ast(&self, uri: &Url) -> Option<Ast> {
        if let Some(ast) = self.ast_map.get(uri) {
            return Some(ast.clone());
        }
        if let Ok(text) = std::fs::read_to_string(uri.path()) {
            let globals = self.collect_globals();
            let ast = fennel_parser::parse(text.chars(), globals);
            self.ast_map.insert(uri.clone(), ast.clone());

            let doc = Rope::from_str(&text);
            self.doc_map.insert(uri.clone(), doc);

            Some(ast)
        } else {
            None
        }
    }

    async fn publish_diagnostics(
        &self,
        doc: &Rope,
        uri: Url,
        ast: &Ast,
        version: Option<i32>,
        on_save_or_open: bool,
    ) {
        if on_save_or_open {
            self.on_save_or_open_errors
                .insert(uri.clone(), ast.on_save_errors().cloned().collect());
        } else if let Some(mut errs) = self.on_save_or_open_errors.get_mut(&uri) {
            let new_errors: Vec<&fennel_parser::Error> = ast.on_save_errors().collect();
            errs.retain(|e| new_errors.contains(&e))
        };

        let errors: Vec<fennel_parser::Error> =
            if let Some(on_save_errors) = self.on_save_or_open_errors.get(&uri) {
                ast.errors().chain(on_save_errors.iter()).cloned().collect()
            } else {
                ast.errors().cloned().collect()
            };

        let diagnostics = errors.into_iter().flat_map(|error| {
            lsp_range(doc, error.range).map(|range| {
                let (message, severity) = view::error(error.kind);
                Diagnostic::new(
                    range,
                    Some(severity),
                    None,
                    Some("Fennel Diagnostics".into()),
                    message,
                    None,
                    None,
                )
            })
        });
        self.client.publish_diagnostics(uri, diagnostics.collect(), version).await;
    }

    fn free_doc(&self, uri: &Url) {
        self.doc_map.remove(uri);
        self.ast_map.remove(uri);
        self.on_save_or_open_errors.remove(uri);
    }

    async fn find_r_symbol_in_ast(
        &self,
        ast: &Ast,
        fields: &[&str],
    ) -> Option<fennel_parser::models::RSymbol> {
        if fields.is_empty() {
            return None;
        }

        let mut fields_iter = fields.iter();
        let mut current_field = fields_iter.next()?;

        let mut kv_table = {
            let root =
                fennel_parser::ast::nodes::Root::cast(SyntaxNode::new_root(ast.root.clone()))?;
            root.return_kv_table()?
        };

        while let Some(eval_ast) = kv_table.get(*current_field) {
            if let Some(next_field) = fields_iter.next() {
                current_field = next_field;
                kv_table = eval_ast.cast_kv_table()?.cast_hashmap();
            } else {
                // Found the last field.
                let syntax = eval_ast.syntax();
                let range = syntax.text_range();

                return Some(fennel_parser::models::RSymbol {
                    token: fennel_parser::models::Token { text: current_field.to_string(), range },
                    special: models::SpecialKind::Normal,
                });
            }
        }

        None
    }

    async fn find_symbol_in_ast(
        &self,
        ast: &Ast,
        fields: &[&str],
    ) -> Option<fennel_parser::models::LSymbol> {
        if fields.is_empty() {
            return None;
        }

        let mut fields_iter = fields.iter();
        let mut current_field = fields_iter.next()?;

        let mut kv_table = {
            let root =
                fennel_parser::ast::nodes::Root::cast(SyntaxNode::new_root(ast.root.clone()))?;
            root.return_kv_table()?
        };

        while let Some(eval_ast) = kv_table.get(*current_field) {
            if let Some(next_field) = fields_iter.next() {
                current_field = next_field;
                kv_table = eval_ast.cast_kv_table()?.cast_hashmap();
            } else {
                // Found the last field.
                let syntax = eval_ast.syntax();
                let range = syntax.text_range();

                // Try to follow if it's a symbol
                if let Some(def) = ast.definition(range.start().into())
                    && let fennel_parser::Definition::Symbol(lsym, _) = def
                {
                    return Some(lsym);
                }

                return Some(fennel_parser::models::LSymbol {
                    token: fennel_parser::models::Token {
                        text: current_field.to_string(),
                        range: syntax.text_range(),
                    },
                    scope: fennel_parser::models::Scope {
                        kind: fennel_parser::models::ScopeKind::Local,
                        range: syntax.text_range(),
                    },
                    value: fennel_parser::models::Value {
                        kind: eval_ast.eval_kind(),
                        range: Some(syntax.text_range()),
                    },
                });
            }
        }

        None
    }

    fn find_file(&self, rel: &Url, path: PathBuf) -> Option<Url> {
        path.to_str()?;

        let check_exist = |rel: &Url, ext: &str, init: bool| -> Option<Url> {
            let path = if init { path.join("init") } else { path.clone() };
            if let Ok(url) = rel.join(path.with_extension(ext).to_str()?)
                && std::fs::metadata(url.path()).map(|m| m.is_file()).unwrap_or(false)
            {
                return Some(url);
            }
            None
        };

        let library = &self
            .config
            .read()
            .expect("config lock should not be poisoned")
            .fennel
            .workspace
            .library;
        let library_file = library.iter().find_map(|uri| {
            let mut uri = uri.0.clone();
            if !uri.path().ends_with('/') {
                uri.path_segments_mut().ok()?.push("");
            }
            let uri_fnl = uri.join("fnl/").ok()?;
            let uri_lua = uri.join("lua/").ok()?;
            check_exist(&uri_lua, "lua", false)
                .or_else(|| check_exist(&uri_lua, "lua", true))
                .or_else(|| check_exist(&uri_fnl, "fnl", false))
                .or_else(|| check_exist(&uri_fnl, "fnl", true))
                .or_else(|| check_exist(&uri, "lua", false))
                .or_else(|| check_exist(&uri, "lua", true))
                .or_else(|| check_exist(&uri, "fnl", false))
                .or_else(|| check_exist(&uri, "fnl", true))
        });

        let workspace_file = self.workspace_map.iter().find_map(|ref r| {
            let mut uri = r.key().clone();
            uri.path_segments_mut().ok()?.push("");
            if !rel.path().starts_with(uri.path()) {
                return None;
            };

            let uri_fnl = uri.join("fnl/").ok()?;
            check_exist(&uri_fnl, "fnl", false).or_else(|| check_exist(&uri_fnl, "fnl", true))
        });

        workspace_file.or(library_file).or_else(|| {
            check_exist(rel, "lua", false)
                .or_else(|| check_exist(rel, "lua", true))
                .or_else(|| check_exist(rel, "so", false))
                .or_else(|| check_exist(rel, "fnl", false))
                .or_else(|| check_exist(rel, "fnl", true))
        })
    }

    fn extract_func_signature(
        &self,
        callee_name: &str,
        callee_offset: u32,
        ast: &Ast,
    ) -> Option<(String, Vec<String>, Option<String>)> {
        let (lsym, is_std) = if let Some(def) = ast.definition(callee_offset) {
            match def {
                fennel_parser::Definition::Symbol(s, _)
                | fennel_parser::Definition::SymbolField(s, _) => (s, false),
                _ => return None,
            }
        } else if let Some(fennel_parser::Definition::Symbol(s, _)) =
            self.std_ast.definition_for_global(callee_name)
        {
            (s, true)
        } else {
            let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
            let s = std_symbols.into_iter().find(|s| s.token.text == callee_name)?;
            (s.clone(), true)
        };

        let (def_ast, def_root) = if is_std {
            (&self.std_ast, SyntaxNode::new_root(self.std_ast.root.clone()))
        } else {
            (ast, SyntaxNode::new_root(ast.root.clone()))
        };

        let def_offset = lsym.token.range.start();
        let token = def_root.token_at_offset(def_offset).right_biased()?;
        let func_node = token.parent_ancestors().find(|n| {
            matches!(n.kind(), SyntaxKind::N_FN | SyntaxKind::N_LAMBDA | SyntaxKind::N_MACRO)
        })?;

        let param_table = func_node.children().find(|n| n.kind() == SyntaxKind::N_PARAM_TABLE)?;

        let mut params = Vec::new();
        for element in param_table.descendants_with_tokens() {
            if let Some(t) = element.into_token()
                && matches!(t.kind(), SyntaxKind::SYMBOL | SyntaxKind::VARARG)
            {
                params.push(t.text().to_string());
            }
        }

        let docstring = def_ast.docstring(lsym.token.range);
        let label = format!("(fn {} [{}])", callee_name, params.join(" "));

        Some((label, params, docstring))
    }

    fn symbol_completion_detail_and_doc(
        &self,
        symbol: &models::LSymbol,
        ast: &Ast,
    ) -> (Option<String>, Option<Documentation>) {
        if matches!(
            symbol.scope.kind,
            models::ScopeKind::Func | models::ScopeKind::Lambda | models::ScopeKind::Macro
        ) {
            let root = SyntaxNode::new_root(ast.root.clone());
            if let Some(token) = root.token_at_offset(symbol.token.range.start()).right_biased()
                && let Some(func_node) = token.parent_ancestors().find(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::N_FN | SyntaxKind::N_LAMBDA | SyntaxKind::N_MACRO
                    )
                })
                && let Some(param_table) =
                    func_node.children().find(|n| n.kind() == SyntaxKind::N_PARAM_TABLE)
            {
                let mut params = Vec::new();
                for element in param_table.descendants_with_tokens() {
                    if let Some(t) = element.into_token()
                        && matches!(t.kind(), SyntaxKind::SYMBOL | SyntaxKind::VARARG)
                    {
                        params.push(t.text().to_string());
                    }
                }
                let sig = format!("(fn {} [{}])", symbol.token.text, params.join(" "));
                let mut doc = ast.docstring(symbol.token.range).map(Documentation::String);
                if doc.is_none()
                    && let Some(body) =
                        func_node.children().find(|n| n.kind() == SyntaxKind::N_BODY)
                {
                    let sexps: Vec<_> =
                        body.children().filter(|n| n.kind() == SyntaxKind::N_SEXP).collect();
                    if sexps.len() == 1
                        && let Some(first_sexp) = sexps.first()
                    {
                        for t in first_sexp.descendants_with_tokens() {
                            if t.kind() == SyntaxKind::QUOTE_STRING {
                                let text = t.to_string();
                                let clean = text
                                    .strip_prefix('"')
                                    .and_then(|s| s.strip_suffix('"'))
                                    .unwrap_or(&text);
                                doc = Some(Documentation::String(clean.to_string()));
                                break;
                            }
                        }
                    }
                }
                return (Some(sig), doc);
            }
        }

        let vk = view::value_kind(&symbol.value.kind);
        let detail =
            if !vk.is_empty() { Some(vk.to_string()) } else { Some(symbol.token.text.clone()) };
        let doc = ast.docstring(symbol.token.range).map(Documentation::String);
        (detail, doc)
    }
}

fn is_selection_node_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::N_LIST
            | SyntaxKind::N_MACRO_LIST
            | SyntaxKind::N_SEQ_TABLE
            | SyntaxKind::N_KV_TABLE
            | SyntaxKind::N_ASSIGN_TABLE
            | SyntaxKind::N_FOR_TABLE
            | SyntaxKind::N_EACH_TABLE
            | SyntaxKind::N_PARAM_TABLE
            | SyntaxKind::N_ICOLLECT_TABLE
            | SyntaxKind::N_COLLECT_TABLE
            | SyntaxKind::N_FCOLLECT_TABLE
            | SyntaxKind::N_ACCUMULATE_TABLE
            | SyntaxKind::N_KV_PAIR
            | SyntaxKind::N_NV_PAIR
            | SyntaxKind::N_ASSIGN_PAIR
            | SyntaxKind::N_SET_PAIR
            | SyntaxKind::ROOT
    )
}

fn is_folding_node_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::N_LIST
            | SyntaxKind::N_MACRO_LIST
            | SyntaxKind::N_SEQ_TABLE
            | SyntaxKind::N_KV_TABLE
            | SyntaxKind::N_ASSIGN_TABLE
            | SyntaxKind::N_FOR_TABLE
            | SyntaxKind::N_EACH_TABLE
            | SyntaxKind::N_PARAM_TABLE
            | SyntaxKind::N_ICOLLECT_TABLE
            | SyntaxKind::N_COLLECT_TABLE
            | SyntaxKind::N_FCOLLECT_TABLE
            | SyntaxKind::N_ACCUMULATE_TABLE
            | SyntaxKind::N_FN
            | SyntaxKind::N_LAMBDA
            | SyntaxKind::N_MACRO
            | SyntaxKind::N_LET
            | SyntaxKind::N_MATCH
            | SyntaxKind::N_WHILE
    )
}

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("!!! PANIC: {}", info);
    }));

    let cli = cli::Cli::parse();
    let (read, write) = match &cli.cmd {
        Some(cli::Command::Lsp { cmd: cli::LspCommand::Stdio }) | None => stdio(),
        Some(cli::Command::Lsp { cmd: cli::LspCommand::Tcp { address } }) => {
            tcp_listen(address).await
        }
    };

    let std_fnl = include_str!("../../../stubs/lua54.fnl");
    let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());
    let std_uri = Url::parse(STUBS_URI).expect("hardcoded stubs URI must be valid");

    let doc_map = DashMap::new();
    doc_map.insert(std_uri.clone(), Rope::from_str(std_fnl));

    let (service, socket) = tower_lsp::LspService::build(|client| Backend {
        client,
        doc_map,
        ast_map: DashMap::new(),
        workspace_map: DashMap::new(),
        on_save_or_open_errors: DashMap::new(),
        config: Arc::new(RwLock::new(config::Configuration::default())),
        std_ast,
        std_uri,
    })
    .finish();

    tower_lsp::Server::new(read, write, socket).serve(service).await;
}

fn stdio() -> (Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>) {
    let (read, write) = (tokio::io::stdin(), tokio::io::stdout());
    (Box::new(read), Box::new(write))
}

async fn tcp_listen(address: &str) -> (Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>) {
    let listener = TcpListener::bind(address).await.expect("failed to bind TCP listener");
    let (stream, _) = listener.accept().await.expect("failed to accept TCP connection");
    let (read, write) = tokio::io::split(stream);
    (Box::new(read), Box::new(write))
}

fn collect_fnl_filepaths(root: Url) -> Result<Vec<Url>> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(root.to_file_path().map_err(|_path| Error::internal_error())?)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.file_name().to_str().map(|s| s.ends_with("fnl")).unwrap_or(false))
        .filter_map(|e| e.ok())
    {
        let f_url = Url::from_file_path(entry.path()).map_err(|_url| Error::internal_error())?;
        paths.push(f_url);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_harness::TestHarness;

    #[tokio::test]
    async fn test_find_telescope_builtin_normalized() {
        let harness = TestHarness::with_temp_dir();
        harness.write_file("telescope.nvim/lua/telescope/builtin.lua", "return {}");
        harness.add_library_dir(&harness.temp_path().join("telescope.nvim"));

        let base_url = Url::parse("file:///dummy.fnl").expect("dummy URL should be valid");
        let target_path = PathBuf::from("telescope/builtin");

        let resolved = harness.backend.find_file(&base_url, target_path);
        assert!(resolved.is_some(), "Should find telescope.builtin in library");
        let resolved_url = resolved.expect("should have resolved URL");
        assert!(resolved_url.path().ends_with("telescope.nvim/lua/telescope/builtin.lua"));
    }

    #[tokio::test]
    async fn test_find_telescope_builtin() {
        let harness = TestHarness::with_temp_dir();
        harness.write_file("telescope.nvim/lua/telescope/builtin.lua", "return {}");
        harness.add_library_dir(&harness.temp_path().join("telescope.nvim"));

        let base_url = Url::parse("file:///dummy.fnl").expect("dummy URL should be valid");
        let target_path = PathBuf::from("telescope/builtin");

        let resolved = harness.backend.find_file(&base_url, target_path);
        assert!(resolved.is_some(), "Should find telescope/builtin in library");
        let resolved_url = resolved.expect("should have resolved URL");
        assert!(resolved_url.path().ends_with("telescope.nvim/lua/telescope/builtin.lua"));
    }

    #[tokio::test]
    async fn test_find_library_file() {
        let harness = TestHarness::with_temp_dir();
        harness.write_file("my-lib/lua/my-lib/init.lua", "return {}");
        harness.add_library_dir(&harness.temp_path().join("my-lib"));

        let base_url = Url::parse("file:///dummy.fnl").expect("dummy URL should be valid");
        let target_path = PathBuf::from("my-lib");

        let resolved = harness.backend.find_file(&base_url, target_path);
        assert!(resolved.is_some());
        let resolved_url = resolved.expect("should have resolved URL");
        assert!(resolved_url.path().ends_with("my-lib/lua/my-lib/init.lua"));
    }

    #[tokio::test]
    async fn test_print_document_symbols() {
        let code = "(local x 1)\n(fn my-func [a] (+ a x))\n(global my-global \"hello\")";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test.fnl", code);

        let response = harness
            .document_symbols(&uri)
            .await
            .expect("document_symbol should not error")
            .expect("should return symbols");

        if let DocumentSymbolResponse::Nested(symbols) = response {
            assert_eq!(symbols.len(), 4);
            assert_eq!(symbols[0].name, "x");
            assert_eq!(symbols[1].name, "my-func");
            assert_eq!(symbols[2].name, "a");
            assert_eq!(symbols[3].name, "my-global");
        } else {
            panic!("expected Nested symbols response");
        }
    }

    #[tokio::test]
    async fn test_goto_definition_field_access() {
        let lib_code = "(fn setup [] (print \"setup\")) {: setup}";
        let main_code = "(local lsp (require :mylib)) (lsp.setup)";

        let mut harness = TestHarness::with_temp_dir();
        let _lib_uri = harness.write_and_open_file("mylib.fnl", lib_code);
        let main_uri = harness.write_and_open_file("main.fnl", main_code);

        let response = harness
            .goto_definition(&main_uri, 0, 34)
            .await
            .expect("goto_definition should not error");
        assert!(response.is_some(), "should find definition");
        if let Some(GotoDefinitionResponse::Array(locations)) = response {
            assert_eq!(locations.len(), 2);
            assert!(locations.iter().any(|loc| loc.uri.path().ends_with("mylib.fnl")));
        } else {
            panic!("expected Array of locations");
        }
    }

    #[tokio::test]
    async fn test_goto_definition_import_macros() {
        let macro_code = "(fn my-macro [] (print \"macro\")) {: my-macro}";
        let main_code = "(import-macros {: my-macro} :mymacros) (my-macro)";

        let mut harness = TestHarness::with_temp_dir();
        let _macro_uri = harness.write_and_open_file("mymacros.fnl", macro_code);
        let main_uri = harness.write_and_open_file("main.fnl", main_code);

        let response = harness
            .goto_definition(&main_uri, 0, 42)
            .await
            .expect("goto_definition should not error");
        assert!(response.is_some(), "should find macro definition");
        if let Some(GotoDefinitionResponse::Scalar(loc)) = response {
            assert!(loc.uri.path().ends_with("mymacros.fnl"));
        } else {
            panic!("expected Scalar location");
        }
    }

    #[tokio::test]
    async fn test_references_from_definition() {
        let code = "\
(fn setup-lsp-highlights [bufnr client]
  (print bufnr client))

(fn on-attach [bufnr]
  (setup-lsp-highlights bufnr nil))";

        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test.fnl", code);

        let locations = harness
            .references(&uri, 0, 4, true)
            .await
            .expect("references from definition should not error")
            .expect("should find references");

        assert!(locations.len() >= 2);
        let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
        assert!(lines.contains(&0));
        assert!(lines.contains(&4));
    }

    #[tokio::test]
    async fn test_references_from_usage() {
        let code = "\
(fn setup-lsp-highlights [bufnr client]
  (print bufnr client))

(fn on-attach [bufnr]
  (setup-lsp-highlights bufnr nil))";

        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test.fnl", code);

        let locations = harness
            .references(&uri, 4, 3, true)
            .await
            .expect("references from usage should not error")
            .expect("should find references");

        assert!(locations.len() >= 2);
    }
}
