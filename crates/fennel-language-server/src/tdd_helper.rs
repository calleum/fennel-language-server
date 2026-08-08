use super::*;
use dashmap::DashMap;
use ropey::Rope;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tower_lsp::lsp_types::*;

pub const STUBS_URI: &str = "fennel-ls://stubs/lua54.fnl";

pub async fn setup_backend(code: &str) -> (Backend, Url) {
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
        std_uri: Url::parse(STUBS_URI).expect("stubs URI should be valid"),
    });
    let backend = service.inner().clone();
    let uri = Url::parse("file:///test.fnl").expect("test URL should be valid");

    backend.doc_map.insert(uri.clone(), Rope::from_str(code));
    let ast = fennel_parser::parse(code.chars(), HashSet::new());
    backend.ast_map.insert(uri.clone(), ast);

    (backend, uri)
}
