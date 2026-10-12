use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use dashmap::{DashMap, DashSet};
use ropey::Rope;
use tempfile::TempDir;
use tower_lsp::{LanguageServer, jsonrpc::Result, lsp_types::*};

use crate::{
    Backend,
    config::{self, Configuration},
};

pub use crate::STUBS_URI;

pub struct TestHarness {
    pub backend: Backend,
    pub temp_dir: Option<TempDir>,
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[expect(dead_code, reason = "test helper methods")]
impl TestHarness {
    pub fn new() -> Self {
        let std_fnl = include_str!("../../../stubs/lua54.fnl");
        let std_ast = fennel_parser::parse(std_fnl.chars(), HashSet::new());
        let std_globals = crate::precompute_std_globals(&std_ast);
        let globals_cache = Arc::new(RwLock::new(Arc::clone(&std_globals)));
        let std_completions = Arc::new(crate::precompute_std_completions(&std_ast));

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            open_docs: DashSet::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(Configuration::default())),
            std_globals,
            globals_cache,
            std_completions,
            std_ast,
            std_uri: Url::parse(STUBS_URI).expect("stubs URI should be valid"),
            client_caps: Arc::new(RwLock::new(ClientCapabilities::default())),
        });

        Self { backend: service.inner().clone(), temp_dir: None }
    }

    pub fn with_temp_dir() -> Self {
        let mut harness = Self::new();
        harness.temp_dir = Some(tempfile::tempdir().expect("should create temp dir"));
        harness
    }

    pub fn temp_path(&self) -> &Path {
        self.temp_dir.as_ref().expect("TestHarness was created without a temp dir").path()
    }

    pub fn open_virtual(&mut self, uri_str: &str, content: &str) -> Url {
        let uri = if uri_str.starts_with("file://") || uri_str.starts_with("fennel://") {
            Url::parse(uri_str).expect("valid URL")
        } else {
            Url::parse(&format!("file:///{}", uri_str.trim_start_matches('/'))).expect("valid URL")
        };

        self.backend.doc_map.insert(uri.clone(), Rope::from_str(content));
        let globals = self.backend.collect_globals_arc();
        let ast = fennel_parser::parse_shared(content.chars(), globals);
        self.backend.ast_map.insert(uri.clone(), ast);

        uri
    }

    pub fn write_file(&self, rel_path: &str, content: &str) -> (PathBuf, Url) {
        let path = self.temp_path().join(rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("should create directories");
        }
        fs::write(&path, content).expect("should write file");
        let url = Url::from_file_path(&path).expect("valid file URL");
        (path, url)
    }

    pub fn write_and_open_file(&mut self, rel_path: &str, content: &str) -> Url {
        let (_path, url) = self.write_file(rel_path, content);
        self.backend.doc_map.insert(url.clone(), Rope::from_str(content));
        self.backend.open_docs.insert(url.clone());
        let globals = self.backend.collect_globals_arc();
        let ast = fennel_parser::parse_shared(content.chars(), globals);
        self.backend.ast_map.insert(url.clone(), ast);
        url
    }

    pub fn add_library_dir(&self, dir_path: &Path) {
        let dir_url = Url::from_directory_path(dir_path).expect("valid directory URL");
        self.backend
            .config
            .write()
            .expect("config lock should not be poisoned")
            .fennel
            .workspace
            .library
            .push(config::Url(dir_url));
    }

    pub async fn goto_definition(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.goto_definition(params).await
    }

    pub async fn references(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Option<Vec<Location>>> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext { include_declaration },
        };
        self.backend.references(params).await
    }

    pub async fn document_symbols(&self, uri: &Url) -> Result<Option<DocumentSymbolResponse>> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.document_symbol(params).await
    }

    pub async fn completion(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Result<Option<CompletionResponse>> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        self.backend.completion(params).await
    }

    pub async fn document_highlight(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let params = DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.document_highlight(params).await
    }

    pub async fn signature_help(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Result<Option<SignatureHelp>> {
        let params = SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        };
        self.backend.signature_help(params).await
    }

    pub async fn workspace_symbols(&self, query: &str) -> Result<Option<Vec<SymbolInformation>>> {
        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.symbol(params).await
    }

    pub async fn semantic_tokens(&self, uri: &Url) -> Result<Option<SemanticTokensResult>> {
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.semantic_tokens_full(params).await
    }

    pub async fn folding_range(&self, uri: &Url) -> Result<Option<Vec<FoldingRange>>> {
        let params = FoldingRangeParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.folding_range(params).await
    }

    pub async fn selection_range(
        &self,
        uri: &Url,
        positions: Vec<Position>,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let params = SelectionRangeParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            positions,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.selection_range(params).await
    }

    pub async fn prepare_rename(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Result<Option<PrepareRenameResponse>> {
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            position: Position::new(line, character),
        };
        self.backend.prepare_rename(params).await
    }

    pub async fn rename(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>> {
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            new_name: new_name.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.backend.rename(params).await
    }

    pub async fn document_link(&self, uri: &Url) -> Result<Option<Vec<DocumentLink>>> {
        let params = DocumentLinkParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.document_link(params).await
    }

    pub async fn hover(&self, uri: &Url, line: u32, character: u32) -> Result<Option<Hover>> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.backend.hover(params).await
    }

    pub async fn inlay_hint(&self, uri: &Url, range: Range) -> Result<Option<Vec<InlayHint>>> {
        let params = InlayHintParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.backend.inlay_hint(params).await
    }

    pub async fn code_action(
        &self,
        uri: &Url,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Option<CodeActionResponse>> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
            range,
            context: CodeActionContext { diagnostics, only: None, trigger_kind: None },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.code_action(params).await
    }

    pub async fn execute_command(
        &self,
        command: &str,
        arguments: Vec<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>> {
        let params = ExecuteCommandParams {
            command: command.to_string(),
            arguments,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.backend.execute_command(params).await
    }

    pub async fn initialize(&self) -> Result<InitializeResult> {
        let temp_path = self.temp_path();
        let root_uri = Url::from_directory_path(temp_path).expect("valid directory URL");
        let params = InitializeParams {
            root_uri: Some(root_uri.clone()),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri,
                name: "root".to_string(),
            }]),
            ..Default::default()
        };
        let res = self.backend.initialize(params).await?;
        self.backend.initialized(InitializedParams {}).await;
        Ok(res)
    }

    pub async fn prepare_call_hierarchy(
        &self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let params = CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(line, character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.backend.prepare_call_hierarchy(params).await
    }

    pub async fn incoming_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let params = CallHierarchyIncomingCallsParams {
            item,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.incoming_calls(params).await
    }

    pub async fn outgoing_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let params = CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.backend.outgoing_calls(params).await
    }

    pub fn get_diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        let doc = self.backend.doc_map.get(uri).expect("doc should exist");
        if crate::is_flsproject(uri) {
            let ast = fennel_parser::parse_shared(doc.chars(), self.backend.collect_globals_arc());
            self.backend.compute_diagnostics(&doc, uri, &ast, true)
        } else {
            let ast = self.backend.ast_map.get(uri).expect("ast should exist");
            self.backend.compute_diagnostics(&doc, uri, &ast, true)
        }
    }

    pub async fn did_save_file(&self, uri: &Url) {
        self.backend
            .did_save(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                text: None,
            })
            .await;
    }

    pub async fn did_change_file(&mut self, uri: &Url, new_text: &str) {
        self.backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier::new(uri.clone(), 2),
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: new_text.to_string(),
                }],
            })
            .await;
    }

    pub async fn reload_flsproject(&self) -> Vec<crate::flsproject::FlsValidationError> {
        self.backend.reload_flsproject().await
    }

    pub async fn did_change_configuration(&self, settings: serde_json::Value) {
        self.backend.did_change_configuration(DidChangeConfigurationParams { settings }).await;
    }
}

pub async fn setup_backend(code: &str) -> (Backend, Url) {
    let mut harness = TestHarness::new();
    let uri = harness.open_virtual("file:///test.fnl", code);
    (harness.backend, uri)
}
