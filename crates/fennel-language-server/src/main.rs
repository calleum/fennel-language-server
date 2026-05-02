mod cli;
mod config;
mod helper;
mod view;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use clap::Parser;
use dashmap::DashMap;
use fennel_parser::{Ast, SyntaxNode, models};
use helper::*;
use ropey::Rope;
use rowan::ast::AstNode;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};
use tower_lsp::{
    jsonrpc::{Error, Result},
    lsp_types::*,
};

use crate::view::{document_symbols_view, value_kind_to_symbol_kind};

const STUBS_URI: &str = "fennel://stubs/lua54.fnl";

#[derive(Debug)]
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
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Initial config from options: {:?}", config),
                )
                .await;
            *self.config.write().unwrap() = config;
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
                rename_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".into(), ":".into()]),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
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
                let base = fields[0];

                if self.std_ast.definition_for_global(base).is_some() {
                    let std_doc = self.doc_map.get(&self.std_uri);
                    let std_range_fn = |range: fennel_parser::TextRange| -> Range {
                        if let Some(doc) = &std_doc {
                            lsp_range(doc, range).unwrap_or_else(|_| lsp_range_head())
                        } else {
                            lsp_range_head()
                        }
                    };

                    if fields.len() > 1 {
                        if let Some(target_lsymbol) =
                            self.find_symbol_in_ast(&self.std_ast, &fields[1..]).await
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

            #[allow(deprecated)]
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

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let offset = position_to_byte_offset(&doc, position)?;

        let references = ast.reference(offset);
        if references.is_none() {
            return Ok(None);
        }
        let references = references.unwrap();
        if references.is_empty() {
            return Err(Error::request_cancelled());
        }
        let mut locations = Vec::with_capacity(references.len());
        for reference in references {
            let range = lsp_range(&doc, reference)?;
            locations.push(Location::new(uri.clone(), range));
        }
        Ok(Some(locations))
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
        let ranges = ast
            .reference(offset)
            .ok_or_else(|| Error::invalid_params("No references found at position"))?;
        if ranges.is_empty() {
            return Ok(None);
        }

        let mut changes = Vec::with_capacity(ranges.len());
        for range in ranges {
            let range = lsp_range(&doc, range)?;
            changes.push(TextEdit::new(range, params.new_name.clone()))
        }
        let mut map = HashMap::new();
        map.insert(uri, changes);
        Ok(Some(WorkspaceEdit::new(map)))
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
            _ => return Ok(None),
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
        let symbols = symbols.map(|symbol| CompletionItem {
            label: symbol.token.text.clone(),
            insert_text: Some(symbol.token.text.clone()),
            kind: Some(view::completion_scope_kind(symbol.scope.kind)),
            detail: Some(symbol.token.text.clone()),
            ..Default::default()
        });

        let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
        let std_completions = std_symbols.map(|symbol| CompletionItem {
            label: symbol.token.text.clone(),
            insert_text: Some(symbol.token.text.clone()),
            kind: Some(view::completion_scope_kind(symbol.scope.kind)),
            detail: Some(symbol.token.text.clone()),
            ..Default::default()
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
                            let field = sym.token.text.strip_prefix(&prefix).unwrap();
                            if !field.contains('.') {
                                completions.push(CompletionItem {
                                    label: field.to_string(),
                                    insert_text: Some(field.to_string()),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(field.to_string()),
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

        let actions = ast.hint_action(offset);
        let res = actions.iter().filter_map(|(range, action)| {
            let range = lsp_range(&doc, *range).ok()?;
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
            Some(action)
        });
        Ok(Some(res.collect()))
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client.log_message(MessageType::INFO, "initialized!").await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client.log_message(MessageType::INFO, "file opened!").await;
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        let doc = ropey::Rope::from_str(&text);
        self.doc_map.insert(uri.clone(), doc.clone());

        let mut globals = HashSet::new();
        for global in &self.config.read().unwrap().fennel.diagnostics.globals {
            globals.insert(global.clone());
        }

        // Add stubs to globals to suppress errors
        let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
        for sym in std_symbols {
            globals.insert(sym.token.text.clone());
        }

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
                let range = rope_range(&doc, lsp_range).unwrap();
                doc.remove(range.clone());
                if !change.text.is_empty() {
                    doc.insert(range.start, &change.text);
                }
            } else {
                *doc = Rope::from_str(&change.text);
            }
        });

        let mut globals = HashSet::new();
        for global in &self.config.read().unwrap().fennel.diagnostics.globals {
            globals.insert(global.clone());
        }

        // Add stubs to globals to suppress errors
        let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
        for sym in std_symbols {
            globals.insert(sym.token.text.clone());
        }

        let ast = fennel_parser::parse(doc.chars(), globals);
        self.publish_diagnostics(&doc, uri.clone(), &ast, Some(version), false).await;

        self.ast_map.insert(uri, ast);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request);
        if ast.is_err() {
            return;
        }
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request);
        if doc.is_err() {
            return;
        }
        self.publish_diagnostics(&doc.unwrap(), uri, &ast.unwrap(), None, true).await;
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
                self.client
                    .log_message(MessageType::INFO, format!("Config updated: {:?}", config))
                    .await;
                *self.config.write().unwrap() = config.clone();

                let mut globals = HashSet::new();
                for global in &config.fennel.diagnostics.globals {
                    globals.insert(global.clone());
                }
                let (std_symbols, _, _, _) =
                    self.std_ast.completion(self.std_ast.end_offset(), None);
                for sym in std_symbols {
                    globals.insert(sym.token.text.clone());
                }

                for mut r in self.ast_map.iter_mut() {
                    let ast = r.value_mut();
                    ast.update_globals(globals.iter().cloned().collect());
                    let uri = r.key();
                    let doc = self.doc_map.get(uri).unwrap();
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
    async fn get_or_parse_ast(&self, uri: &Url) -> Option<Ast> {
        if let Some(ast) = self.ast_map.get(uri) {
            return Some(ast.clone());
        }
        if let Ok(text) = std::fs::read_to_string(uri.path()) {
            let mut globals = HashSet::new();
            for global in &self.config.read().unwrap().fennel.diagnostics.globals {
                globals.insert(global.clone());
            }
            let (std_symbols, _, _, _) = self.std_ast.completion(self.std_ast.end_offset(), None);
            for sym in std_symbols {
                globals.insert(sym.token.text.clone());
            }
            let ast = fennel_parser::parse(text.chars(), globals);
            self.ast_map.insert(uri.clone(), ast.clone());
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
            if let Ok(url) = rel.join(path.with_extension(ext).to_str().unwrap())
                && std::fs::metadata(url.path()).map(|m| m.is_file()).unwrap_or(false)
            {
                return Some(url);
            }
            None
        };

        let library = &self.config.read().unwrap().fennel.workspace.library;
        let library_file = library.iter().find_map(|uri| {
            let mut uri = uri.0.clone();
            if !uri.path().ends_with('/') {
                uri.path_segments_mut().ok()?.push("");
            }
            let uri_fnl = uri.join("fnl/").unwrap();
            let uri_lua = uri.join("lua/").unwrap();
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

            let uri_fnl = uri.join("fnl/").unwrap();
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
}

#[tokio::main]
async fn main() {
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
    let listener = TcpListener::bind(address).await.unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    let (read, write) = tokio::io::split(stream);
    (Box::new(read), Box::new(write))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    use tower_lsp::LanguageServer;

    #[tokio::test]
    async fn test_find_telescope_builtin_normalized() {
        let dir = tempdir().unwrap();
        let lib_path = dir.path().join("telescope.nvim");
        let lua_dir = lib_path.join("lua/telescope");
        fs::create_dir_all(&lua_dir).unwrap();
        let builtin_lua = lua_dir.join("builtin.lua");
        fs::write(&builtin_lua, "return {}").unwrap();

        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(config::Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).unwrap(),
        });
        let backend = service.inner();

        // Configure the library path
        let lib_url = Url::from_directory_path(&lib_path).unwrap();
        backend.config.write().unwrap().fennel.workspace.library = vec![config::Url(lib_url)];

        let base_url = Url::parse("file:///dummy.fnl").unwrap();
        // This is what fennel-parser produces for (require :telescope.builtin)
        let target_path = PathBuf::from("telescope/builtin");

        let resolved = backend.find_file(&base_url, target_path);
        assert!(resolved.is_some(), "Should find telescope.builtin in library");
        let resolved_url = resolved.unwrap();
        assert!(resolved_url.path().ends_with("telescope.nvim/lua/telescope/builtin.lua"));
    }

    #[tokio::test]
    async fn test_find_telescope_builtin() {
        let dir = tempdir().unwrap();
        let lib_path = dir.path().join("telescope.nvim");
        let lua_dir = lib_path.join("lua/telescope");
        fs::create_dir_all(&lua_dir).unwrap();
        let builtin_lua = lua_dir.join("builtin.lua");
        fs::write(&builtin_lua, "return {}").unwrap();

        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(config::Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).unwrap(),
        });
        let backend = service.inner();

        // Configure the library path
        let lib_url = Url::from_directory_path(&lib_path).unwrap();
        backend.config.write().unwrap().fennel.workspace.library = vec![config::Url(lib_url)];

        let base_url = Url::parse("file:///dummy.fnl").unwrap();
        let target_path = PathBuf::from("telescope/builtin");

        let resolved = backend.find_file(&base_url, target_path);
        assert!(resolved.is_some(), "Should find telescope/builtin in library");
        let resolved_url = resolved.unwrap();
        assert!(resolved_url.path().ends_with("telescope.nvim/lua/telescope/builtin.lua"));
    }

    #[tokio::test]
    async fn test_find_library_file() {
        let dir = tempdir().unwrap();
        let lib_path = dir.path().join("my-lib");
        let lua_dir = lib_path.join("lua/my-lib");
        fs::create_dir_all(&lua_dir).unwrap();
        let init_lua = lua_dir.join("init.lua");
        fs::write(&init_lua, "return {}").unwrap();

        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(config::Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).unwrap(),
        });
        let backend = service.inner();

        // Configure the library path
        let lib_url = Url::from_directory_path(&lib_path).unwrap();
        backend.config.write().unwrap().fennel.workspace.library = vec![config::Url(lib_url)];

        let base_url = Url::parse("file:///dummy.fnl").unwrap();
        let target_path = PathBuf::from("my-lib");

        let resolved = backend.find_file(&base_url, target_path);
        assert!(resolved.is_some());
        let resolved_url = resolved.unwrap();
        assert!(resolved_url.path().ends_with("my-lib/lua/my-lib/init.lua"));
    }

    #[tokio::test]
    async fn test_print_document_symbols() {
        let code = "(local x 1)\n(fn my-func [a] (+ a x))\n(global my-global \"hello\")";

        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(config::Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).unwrap(),
        });
        let backend = service.inner();
        let uri = Url::parse("file:///test.fnl").unwrap();

        backend.doc_map.insert(uri.clone(), Rope::from_str(code));
        let ast = fennel_parser::parse(code.chars(), HashSet::new());
        backend.ast_map.insert(uri.clone(), ast);

        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier::new(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.document_symbol(params).await.unwrap().unwrap();

        println!("\n--- Document Symbols Output ---");
        println!("{code}");
        fn print_symbol(sym: &DocumentSymbol, indent: usize) {
            println!(
                "{:indent$}Symbol: {:<12} | Kind: {:?} | Range: {:?}..{:?}",
                "",
                sym.name,
                sym.kind,
                sym.range.start,
                sym.range.end,
                indent = indent
            );
            if let Some(children) = &sym.children {
                for child in children {
                    print_symbol(child, indent + 2);
                }
            }
        }

        if let DocumentSymbolResponse::Nested(symbols) = response {
            for sym in symbols {
                print_symbol(&sym, 0);
            }
        }
        println!("-------------------------------\n");
    }

    #[tokio::test]
    async fn test_goto_definition_field_access() {
        let lib_code = "(fn setup [] (print \"setup\")) {: setup}";
        let main_code = "(local lsp (require :mylib)) (lsp.setup)";

        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(config::Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).unwrap(),
        });
        let backend = service.inner();

        let dir = tempdir().unwrap();
        let lib_path = dir.path().join("mylib.fnl");
        fs::write(&lib_path, lib_code).unwrap();
        let lib_uri = Url::from_file_path(&lib_path).unwrap();
        backend.doc_map.insert(lib_uri.clone(), Rope::from_str(lib_code));

        let main_path = dir.path().join("main.fnl");
        fs::write(&main_path, main_code).unwrap();
        let main_uri = Url::from_file_path(&main_path).unwrap();

        backend.doc_map.insert(main_uri.clone(), Rope::from_str(main_code));
        let main_ast = fennel_parser::parse(main_code.chars(), HashSet::new());
        backend.ast_map.insert(main_uri.clone(), main_ast);

        // lsp.setup
        // 0123456789012345678901234567890123456789
        // (local lsp (require :mylib)) (lsp.setup)
        //                              ^   ^
        //                              30  34

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(main_uri.clone()),
                position: Position::new(0, 34), // on "setup"
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.goto_definition(params).await.unwrap();
        println!("\n--- Goto Definition Field Access ---");
        if let Some(GotoDefinitionResponse::Array(locations)) = response {
            for loc in locations {
                println!("Location: {:?} range: {:?}", loc.uri, loc.range);
            }
        } else if let Some(GotoDefinitionResponse::Scalar(loc)) = response {
            println!("Scalar Location: {:?} range: {:?}", loc.uri, loc.range);
        } else {
            println!("No definition found");
        }
    }

    #[tokio::test]
    async fn test_goto_definition_import_macros() {
        let macro_code = "(fn my-macro [] (print \"macro\")) {: my-macro}";
        let main_code = "(import-macros {: my-macro} :mymacros) (my-macro)";

        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(config::Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).unwrap(),
        });
        let backend = service.inner();

        let dir = tempdir().unwrap();
        let macro_path = dir.path().join("mymacros.fnl");
        fs::write(&macro_path, macro_code).unwrap();
        let macro_uri = Url::from_file_path(&macro_path).unwrap();
        backend.doc_map.insert(macro_uri.clone(), Rope::from_str(macro_code));

        let main_path = dir.path().join("main.fnl");
        fs::write(&main_path, main_code).unwrap();
        let main_uri = Url::from_file_path(&main_path).unwrap();

        backend.doc_map.insert(main_uri.clone(), Rope::from_str(main_code));
        let main_ast = fennel_parser::parse(main_code.chars(), HashSet::new());
        backend.ast_map.insert(main_uri.clone(), main_ast);

        // my-macro
        // 012345678901234567890123456789012345678901234567
        // (import-macros {: my-macro} :mymacros) (my-macro)
        //                                         ^
        //                                         40

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(main_uri.clone()),
                position: Position::new(0, 42), // on "my-macro"
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.goto_definition(params).await.unwrap();
        println!("\n--- Goto Definition Import Macros ---");
        if let Some(GotoDefinitionResponse::Array(locations)) = response {
            for loc in locations {
                println!("Location: {:?} range: {:?}", loc.uri, loc.range);
            }
        } else if let Some(GotoDefinitionResponse::Scalar(loc)) = response {
            println!("Scalar Location: {:?} range: {:?}", loc.uri, loc.range);
        } else {
            println!("No definition found");
        }
    }
}
