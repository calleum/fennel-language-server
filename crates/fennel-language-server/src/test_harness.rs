use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use dashmap::DashMap;
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

        let (service, _) = tower_lsp::LspService::new(|client| Backend {
            client,
            doc_map: DashMap::new(),
            ast_map: DashMap::new(),
            workspace_map: DashMap::new(),
            on_save_or_open_errors: DashMap::new(),
            config: Arc::new(RwLock::new(Configuration::default())),
            std_ast,
            std_uri: Url::parse(STUBS_URI).expect("stubs URI should be valid"),
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
        let globals = self.backend.collect_globals();
        let ast = fennel_parser::parse(content.chars(), globals);
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
        let globals = self.backend.collect_globals();
        let ast = fennel_parser::parse(content.chars(), globals);
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
}

pub async fn setup_backend(code: &str) -> (Backend, Url) {
    let mut harness = TestHarness::new();
    let uri = harness.open_virtual("file:///test.fnl", code);
    (harness.backend, uri)
}
