mod cli;
mod config;
pub(crate) mod flsproject;
mod helper;
mod providers;
mod semantic_tokens;
mod view;

#[cfg(test)]
mod benchmarks;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tdd_helper;
#[cfg(test)]
mod tdd_tests;
#[cfg(test)]
pub(crate) mod test_harness;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use clap::Parser;
use dashmap::{DashMap, DashSet};
use fennel_parser::{Ast, SyntaxKind, SyntaxNode, models};
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
use walkdir::WalkDir;

pub const STUBS_URI: &str = "fennel://stubs/lua54.fnl";

#[derive(Debug, Clone)]
struct Backend {
    client: tower_lsp::Client,
    config: Arc<RwLock<config::Configuration>>,
    doc_map: DashMap<Url, Rope>,
    ast_map: DashMap<Url, Ast>,
    /// URIs the editor currently has open. `doc_map` also holds files read from
    /// disk by background indexing, so it cannot distinguish open buffers from
    /// indexed ones on its own — this set can.
    open_docs: DashSet<Url>,
    workspace_map: DashMap<Url, String>,
    // publish those after saving
    on_save_or_open_errors: DashMap<Url, Vec<fennel_parser::Error>>,
    std_ast: Ast,
    std_uri: Url,
    std_globals: Arc<HashSet<String>>,
    globals_cache: Arc<RwLock<Arc<HashSet<String>>>>,
    std_completions: Arc<Vec<CompletionItem>>,
    /// Captured from `initialize`; `InitializedParams` carries nothing, so the
    /// client capabilities have to be retained to register file watchers later.
    /// Interior mutability because `initialize` takes `&self`.
    client_caps: Arc<RwLock<ClientCapabilities>>,
}

#[tower_lsp::async_trait]
impl tower_lsp::LanguageServer for Backend {
    // ---- Providers -------------------------------------------------
    // Implementations live under `providers::*`; these forward so the trait
    // impl itself stays in one place.

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<CodeActionResponse>> {
        self.code_action(params).await
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<CompletionResponse>> {
        self.completion(params).await
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.did_change(params).await
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.did_change_configuration(params).await
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.did_change_watched_files(params).await
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        self.did_change_workspace_folders(params).await
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.did_close(params).await
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.did_open(params).await
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.did_save(params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<DocumentHighlight>>> {
        self.document_highlight(params).await
    }

    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<DocumentLink>>> {
        self.document_link(params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> tower_lsp::jsonrpc::Result<Option<DocumentSymbolResponse>> {
        self.document_symbol(params).await
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> tower_lsp::jsonrpc::Result<Option<serde_json::Value>> {
        self.execute_command(params).await
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<FoldingRange>>> {
        self.folding_range(params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<GotoDefinitionResponse>> {
        self.goto_definition(params).await
    }

    async fn hover(&self, params: HoverParams) -> tower_lsp::jsonrpc::Result<Option<Hover>> {
        self.hover(params).await
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> tower_lsp::jsonrpc::Result<Option<Vec<CallHierarchyIncomingCall>>> {
        self.incoming_calls(params).await
    }

    async fn initialized(&self, params: InitializedParams) {
        self.initialized(params).await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        self.inlay_hint(params).await
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        self.outgoing_calls(params).await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        self.prepare_call_hierarchy(params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        self.prepare_rename(params).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        self.references(params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        self.rename(params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        self.selection_range(params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        self.semantic_tokens_full(params).await
    }

    async fn shutdown(&self) -> Result<()> {
        self.shutdown().await
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        self.signature_help(params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.symbol(params).await
    }

    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(folders) = params.workspace_folders {
            folders.into_iter().for_each(|folder| {
                self.workspace_map.insert(folder.uri, folder.name);
            });
        } else if let Some(uri) = params.root_uri {
            self.workspace_map.insert(uri, "root".to_string());
        }

        if let Some(settings) = params.initialization_options
            && let Ok(config) = serde_json::from_value::<config::Configuration>(settings)
        {
            *self.config.write().map_err(|_err| Error::internal_error())? = config;
        }

        *self.client_caps.write().map_err(|_err| Error::internal_error())? = params.capabilities;

        // Workspace indexing is deliberately deferred to `initialized` so this
        // handler returns promptly; clients time out slow `initialize` calls.

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
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["fennel.addGlobal".into(), "fennel.expandMacro".into()],
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
}

pub(crate) fn is_flsproject(uri: &Url) -> bool {
    let path = uri.path();
    path.ends_with("/flsproject.fnl") || path.ends_with("flsproject.fnl")
}

impl Backend {
    pub(crate) fn supports_workspace_configuration(&self) -> bool {
        self.client_caps
            .read()
            .ok()
            .map(|caps| caps.workspace.as_ref().and_then(|w| w.configuration).unwrap_or(false))
            .unwrap_or(false)
    }

    pub(crate) fn is_lint_enabled(&self, lint: &str) -> bool {
        self.config.read().map(|c| c.is_lint_enabled(lint)).unwrap_or(true)
    }

    pub(crate) fn recompute_globals(&self) {
        let mut globals = (*self.std_globals).clone();
        if let Ok(config) = self.config.read() {
            for global in &config.fennel.diagnostics.globals {
                globals.insert(global.clone());
            }
            for global in config.fls.extra_globals.split_whitespace() {
                globals.insert(global.to_string());
            }
        }
        if let Ok(mut cache) = self.globals_cache.write() {
            *cache = Arc::new(globals);
        }
    }

    pub(crate) fn collect_globals_arc(&self) -> Arc<HashSet<String>> {
        self.globals_cache
            .read()
            .map(|c| Arc::clone(&*c))
            .unwrap_or_else(|_| Arc::clone(&self.std_globals))
    }

    #[expect(dead_code, reason = "fallback helper for owned globals")]
    pub(crate) fn collect_globals(&self) -> HashSet<String> {
        (*self.collect_globals_arc()).clone()
    }

    pub(crate) async fn ensure_ast(&self, uri: &Url) {
        if self.ast_map.contains_key(uri) {
            return;
        }
        let Ok(file_path) = uri.to_file_path() else {
            return;
        };
        if let Ok(text) = std::fs::read_to_string(&file_path) {
            let globals = self.collect_globals_arc();
            let ast = fennel_parser::parse_shared(text.chars(), globals);
            let doc = Rope::from_str(&text);
            self.doc_map.insert(uri.clone(), doc);
            self.ast_map.insert(uri.clone(), ast);
        }
    }

    pub(crate) async fn index_workspace_folder(&self, folder_uri: &Url) {
        if let Ok(files) = collect_fnl_filepaths(folder_uri.clone()) {
            for f in &files {
                self.ensure_ast(f).await;
            }
        }
    }

    pub(crate) fn uri_to_module_name(&self, file_uri: &Url) -> Option<String> {
        let file_path = file_uri.to_file_path().ok()?;

        // Collect every candidate root. Nested workspace folders overlap, so we
        // must pick the *longest* matching prefix: taking the first match would
        // make the result depend on DashMap iteration order.
        let mut roots: Vec<PathBuf> =
            self.workspace_map.iter().filter_map(|r| r.key().to_file_path().ok()).collect();
        if let Ok(config) = self.config.read() {
            roots.extend(
                config.fennel.workspace.library.iter().filter_map(|lib| lib.0.to_file_path().ok()),
            );
        }

        let mut best: Option<(usize, String)> = None;
        for root in roots {
            let Ok(rel) = file_path.strip_prefix(&root) else {
                continue;
            };
            // Normalize separators *before* stripping. `to_string_lossy` yields
            // backslashes on Windows, so stripping a literal "fnl/" prefix
            // first would silently never match there and yield module names
            // like `fnl.foo` instead of `foo`.
            let Some(mod_name) = module_name_from_relative(rel) else {
                continue;
            };
            let specificity = root.as_os_str().len();
            if best.as_ref().is_none_or(|(len, _)| specificity > *len) {
                best = Some((specificity, mod_name));
            }
        }

        best.map(|(_, mod_name)| mod_name)
            .or_else(|| file_path.file_stem().map(|s| s.to_string_lossy().to_string()))
    }

    /// Ask the client to watch `**/*.fnl` so we learn about edits made outside
    /// the editor.
    ///
    /// lsp-types 0.94 exposes no `ServerCapabilities` field for watched files,
    /// so this has to be registered dynamically. Clients that don't advertise
    /// `didChangeWatchedFiles` dynamic registration simply never send the
    /// notification, and behaviour degrades to the previous open-buffer-only
    /// caching rather than erroring.
    pub(crate) async fn register_file_watchers(&self) {
        let dynamic_ok = self
            .client_caps
            .read()
            .ok()
            .and_then(|caps| {
                caps.workspace
                    .as_ref()
                    .and_then(|w| w.did_change_watched_files.as_ref())
                    .and_then(|d| d.dynamic_registration)
            })
            .unwrap_or(false);

        if !dynamic_ok {
            self.client
                .log_message(
                    MessageType::INFO,
                    "client does not support dynamic file watching; workspace files \
                     only refresh when opened in the editor",
                )
                .await;
            return;
        }

        let options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.fnl".to_string()),
                kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
            }],
        };

        let register_options = serde_json::to_value(options).unwrap_or(serde_json::Value::Null);

        if let Err(e) = self
            .client
            .register_capability(vec![Registration {
                id: "fennel-workspace-fnl-watcher".to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: Some(register_options),
            }])
            .await
        {
            self.client
                .log_message(MessageType::ERROR, format!("failed to register file watcher: {e}"))
                .await;
        }
    }

    /// Re-reads `uri` from disk and replaces any cached AST/document.
    ///
    /// Returns `true` when the cached entry actually changed. Files that vanished
    /// or became unreadable have their entries dropped so the cache cannot serve a
    /// stale copy forever.
    pub(crate) async fn reindex_from_disk(&self, uri: &Url) -> bool {
        let Some(file_path) = uri.to_file_path().ok() else {
            return false;
        };

        let Ok(text) = std::fs::read_to_string(&file_path) else {
            // Deleted or unreadable: drop whatever we cached. Both maps must be
            // cleared — a short-circuiting `||` would skip the second removal.
            let had_ast = self.ast_map.remove(uri).is_some();
            let had_doc = self.doc_map.remove(uri).is_some();
            self.on_save_or_open_errors.remove(uri);
            return had_ast || had_doc;
        };

        // Nothing to do if the on-disk contents match what we already hold.
        if let Some(cached) = self.doc_map.get(uri)
            && cached.value() == text.as_str()
        {
            return false;
        }

        let globals = self.collect_globals_arc();
        let ast = fennel_parser::parse_shared(text.chars(), globals);
        self.ast_map.insert(uri.clone(), ast);
        self.doc_map.insert(uri.clone(), Rope::from_str(&text));
        true
    }

    pub(crate) async fn get_or_parse_ast(&self, uri: &Url) -> Option<Ast> {
        if let Some(ast) = self.ast_map.get(uri) {
            return Some(ast.clone());
        }
        let file_path = uri.to_file_path().ok()?;
        if let Ok(text) = std::fs::read_to_string(&file_path) {
            let globals = self.collect_globals_arc();
            let ast = fennel_parser::parse_shared(text.chars(), globals);
            self.ast_map.insert(uri.clone(), ast.clone());

            let doc = Rope::from_str(&text);
            self.doc_map.insert(uri.clone(), doc);

            Some(ast)
        } else {
            None
        }
    }

    /// Builds the diagnostics for `uri` without awaiting, so callers can drop
    /// their `doc_map` / `ast_map` guards before doing the client round-trip.
    pub(crate) fn compute_diagnostics(
        &self,
        doc: &Rope,
        uri: &Url,
        ast: &Ast,
        on_save_or_open: bool,
    ) -> Vec<Diagnostic> {
        if is_flsproject(uri) {
            if !self.is_lint_enabled("invalid-flsproject-settings") {
                return Vec::new();
            }
            let (_, errors) = flsproject::parse_and_validate_flsproject(&doc.to_string());
            return errors
                .into_iter()
                .filter_map(|error| {
                    lsp_range(doc, error.range).ok().map(|range| {
                        Diagnostic::new(
                            range,
                            Some(DiagnosticSeverity::WARNING),
                            None,
                            Some("Fennel Diagnostics".into()),
                            error.message,
                            None,
                            None,
                        )
                    })
                })
                .collect();
        }

        if on_save_or_open {
            self.on_save_or_open_errors
                .insert(uri.clone(), ast.on_save_errors().cloned().collect());
        } else if let Some(mut errs) = self.on_save_or_open_errors.get_mut(uri) {
            let new_errors: Vec<&fennel_parser::Error> = ast.on_save_errors().collect();
            errs.retain(|e| new_errors.contains(&e))
        };

        let errors: Vec<fennel_parser::Error> =
            if let Some(on_save_errors) = self.on_save_or_open_errors.get(uri) {
                ast.errors().chain(on_save_errors.iter()).cloned().collect()
            } else {
                ast.errors().cloned().collect()
            };

        errors
            .into_iter()
            .filter(|error| {
                if error.kind == fennel_parser::ErrorKind::Unused {
                    self.is_lint_enabled("unused-definition")
                } else {
                    true
                }
            })
            .filter_map(|error| {
                lsp_range(doc, error.range).ok().map(|range| {
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
            })
            .collect()
    }

    pub(crate) async fn publish_diagnostics(
        &self,
        doc: &Rope,
        uri: Url,
        ast: &Ast,
        version: Option<i32>,
        on_save_or_open: bool,
    ) {
        let diagnostics = self.compute_diagnostics(doc, &uri, ast, on_save_or_open);
        self.client.publish_diagnostics(uri, diagnostics, version).await;
    }

    fn free_doc(&self, uri: &Url) {
        self.open_docs.remove(uri);
        self.doc_map.remove(uri);
        self.ast_map.remove(uri);
        self.on_save_or_open_errors.remove(uri);
    }

    pub(crate) async fn find_r_symbol_in_ast(
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

    pub(crate) async fn find_symbol_in_ast(
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

    pub(crate) fn find_file(&self, rel: &Url, path: PathBuf) -> Option<Url> {
        let mod_str = path.to_str()?;

        let (fennel_path, macro_path) = if let Ok(config) = self.config.read() {
            (config.fls.fennel_path.clone(), config.fls.macro_path.clone())
        } else {
            (
                "./?.fnl;./?/init.fnl;src/?.fnl;src/?/init.fnl".to_string(),
                "./?.fnlm;./?/init.fnlm;./?.fnl;./?/init-macros.fnl;./?/init.fnl;src/?.fnlm;src/?/init.fnlm;src/?.fnl;src/?/init-macros.fnl;src/?/init.fnl".to_string(),
            )
        };

        let file_exists = |url: &Url| -> bool {
            if self.doc_map.contains_key(url) {
                return true;
            }
            url.to_file_path()
                .ok()
                .is_some_and(|p| std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false))
        };

        let resolve_pattern = |base_path: &Path, pattern: &str| -> Option<Url> {
            if pattern.is_empty() {
                return None;
            }
            let substituted = pattern.replace('?', mod_str);
            let sub_path = Path::new(&substituted);
            let target_path = if sub_path.is_absolute() {
                sub_path.to_path_buf()
            } else {
                base_path.join(sub_path)
            };
            if target_path.is_file() {
                return Url::from_file_path(target_path).ok();
            }
            let url = Url::from_file_path(&target_path).ok()?;
            if self.doc_map.contains_key(&url) { Some(url) } else { None }
        };

        let mut candidate_roots: Vec<(Url, PathBuf)> = Vec::new();
        let mut matching_ws: Vec<(Url, PathBuf)> = self
            .workspace_map
            .iter()
            .filter(|r| rel.path().starts_with(r.key().path()))
            .filter_map(|r| r.key().to_file_path().ok().map(|p| (r.key().clone(), p)))
            .collect();
        if !matching_ws.is_empty() {
            candidate_roots.append(&mut matching_ws);
        } else {
            candidate_roots.extend(
                self.workspace_map
                    .iter()
                    .filter_map(|r| r.key().to_file_path().ok().map(|p| (r.key().clone(), p))),
            );
        }
        if let Ok(rel_path) = rel.to_file_path()
            && let Some(parent) = rel_path.parent()
            && let Ok(parent_url) = Url::from_directory_path(parent)
            && !candidate_roots.iter().any(|(u, _)| u == &parent_url)
        {
            candidate_roots.push((parent_url, parent.to_path_buf()));
        }

        for (_, base_path) in &candidate_roots {
            for pattern in fennel_path.split(';') {
                if let Some(url) = resolve_pattern(base_path, pattern) {
                    return Some(url);
                }
            }
        }

        for (_, base_path) in &candidate_roots {
            for pattern in macro_path.split(';') {
                if let Some(url) = resolve_pattern(base_path, pattern) {
                    return Some(url);
                }
            }
        }

        let check_exist = |rel: &Url, ext: &str, init: bool| -> Option<Url> {
            let path = if init { path.join("init") } else { path.clone() };
            // `Url::path()` is not a filesystem path: on Windows a file URL's
            // path is `/C:/...`, which `std::fs::metadata` rejects. Convert to
            // a real path first. For non-file URLs this yields `None`, same as
            // the failed metadata call did before.
            if let Ok(url) = rel.join(path.with_extension(ext).to_str()?)
                && file_exists(&url)
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
            check_exist(&uri_fnl, "fnl", false)
                .or_else(|| check_exist(&uri_fnl, "fnl", true))
                .or_else(|| check_exist(&uri, "fnl", false))
                .or_else(|| check_exist(&uri, "fnl", true))
        });

        workspace_file.or(library_file).or_else(|| {
            check_exist(rel, "lua", false)
                .or_else(|| check_exist(rel, "lua", true))
                .or_else(|| check_exist(rel, "so", false))
                .or_else(|| check_exist(rel, "fnl", false))
                .or_else(|| check_exist(rel, "fnl", true))
        })
    }

    pub(crate) fn extract_func_signature(
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

    pub(crate) fn symbol_completion_detail_and_doc(
        &self,
        symbol: &models::LSymbol,
        ast: &Ast,
    ) -> (Option<String>, Option<Documentation>) {
        extract_symbol_completion_detail_and_doc(symbol, ast)
    }
}

pub(crate) fn extract_symbol_completion_detail_and_doc(
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
                matches!(n.kind(), SyntaxKind::N_FN | SyntaxKind::N_LAMBDA | SyntaxKind::N_MACRO)
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
                && let Some(body) = func_node.children().find(|n| n.kind() == SyntaxKind::N_BODY)
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

pub(crate) fn precompute_std_globals(std_ast: &Ast) -> Arc<HashSet<String>> {
    let (std_symbols, _, _, _) = std_ast.completion(std_ast.end_offset(), None);
    Arc::new(std_symbols.map(|sym| sym.token.text.clone()).collect())
}

pub(crate) fn precompute_std_completions(std_ast: &Ast) -> Vec<CompletionItem> {
    let (std_symbols, _, _, _) = std_ast.completion(std_ast.end_offset(), None);
    std_symbols
        .map(|symbol| {
            let (detail, documentation) = extract_symbol_completion_detail_and_doc(symbol, std_ast);
            CompletionItem {
                label: symbol.token.text.clone(),
                insert_text: Some(symbol.token.text.clone()),
                kind: Some(view::completion_scope_kind(symbol.scope.kind)),
                detail,
                documentation,
                ..Default::default()
            }
        })
        .collect()
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
    let std_globals = precompute_std_globals(&std_ast);
    let globals_cache = Arc::new(RwLock::new(Arc::clone(&std_globals)));
    let std_completions = Arc::new(precompute_std_completions(&std_ast));

    let doc_map = DashMap::new();
    doc_map.insert(std_uri.clone(), Rope::from_str(std_fnl));

    let (service, socket) = tower_lsp::LspService::build(|client| Backend {
        client,
        doc_map,
        ast_map: DashMap::new(),
        open_docs: DashSet::new(),
        workspace_map: DashMap::new(),
        on_save_or_open_errors: DashMap::new(),
        config: Arc::new(RwLock::new(config::Configuration::default())),
        std_globals,
        globals_cache,
        std_completions,
        std_ast,
        std_uri,
        client_caps: Arc::new(RwLock::new(ClientCapabilities::default())),
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
    let root_path = root.to_file_path().map_err(|_path| Error::internal_error())?;
    for entry in WalkDir::new(root_path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.')
            } else {
                e.file_name().to_str().is_some_and(|s| s.ends_with(".fnl"))
            }
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file()
            && let Ok(f_url) = Url::from_file_path(entry.path())
        {
            paths.push(f_url);
        }
    }
    Ok(paths)
}

/// Derives a Fennel module name from a path relative to a workspace root.
///
/// Returns `None` when the path carries no module component (e.g. the root's
/// own `init.fnl`).
///
/// Separators are normalized *before* the prefix/suffix rules are applied.
/// `Path::to_string_lossy` yields backslashes on Windows, so stripping a
/// literal `"fnl/"` first would silently never match there and yield module
/// names like `fnl.foo` instead of `foo`.
pub(crate) fn module_name_from_relative(rel: &std::path::Path) -> Option<String> {
    let normalized = rel.to_string_lossy().replace('\\', "/");
    let stripped = normalized.strip_prefix("fnl/").unwrap_or(&normalized);
    let stripped = stripped.strip_suffix(".fnl").unwrap_or(stripped);
    let stripped = stripped.strip_suffix("/init").unwrap_or(stripped);
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.replace('/', "."))
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
