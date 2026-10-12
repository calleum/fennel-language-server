//! Document and workspace lifecycle notifications.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::sync::Arc;

use ropey::Rope;
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, config, helper::*};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn initialized(&self, _: InitializedParams) {
        self.client.log_message(MessageType::LOG, "initialized!").await;
        self.register_file_watchers().await;
        self.pull_configuration().await;
        self.reload_flsproject().await;
        let folders: Vec<Url> = self.workspace_map.iter().map(|r| r.key().clone()).collect();
        for fol in folders {
            self.index_workspace_folder(&fol).await;
        }
        let libraries: Vec<Url> = if let Ok(config) = self.config.read() {
            config.fennel.workspace.library.iter().map(|u| u.0.clone()).collect()
        } else {
            Vec::new()
        };
        for lib in libraries {
            self.index_workspace_folder(&lib).await;
        }
    }

    pub(crate) async fn pull_configuration(&self) {
        if !self.supports_workspace_configuration() {
            return;
        }

        let items = vec![
            ConfigurationItem { scope_uri: None, section: Some("fennel".to_string()) },
            ConfigurationItem { scope_uri: None, section: Some("fennel-ls".to_string()) },
        ];

        if let Ok(configs) = self.client.configuration(items).await {
            self.apply_pulled_configuration(&configs).await;
        }
    }

    pub(crate) async fn apply_pulled_configuration(&self, configs: &[serde_json::Value]) {
        let mut libraries_to_index = Vec::new();
        {
            let Ok(mut config) = self.config.write() else { return };
            if let Some(fennel_val) = configs.first()
                && !fennel_val.is_null()
                && let Ok(fennel) = serde_json::from_value::<config::Fennel>(fennel_val.clone())
            {
                libraries_to_index = fennel.workspace.library.iter().map(|u| u.0.clone()).collect();
                config.fennel = fennel;
            }
            if let Some(fls_val) = configs.get(1)
                && !fls_val.is_null()
                && let Ok(fls) =
                    serde_json::from_value::<crate::flsproject::FlsProjectConfig>(fls_val.clone())
            {
                config.fls = fls;
            }
        }

        for lib in libraries_to_index {
            self.index_workspace_folder(&lib).await;
        }

        self.refresh_ast_globals_and_diagnostics().await;
    }

    pub(crate) async fn refresh_ast_globals_and_diagnostics(&self) {
        self.recompute_globals();
        let globals = self.collect_globals_arc();
        let updated: Vec<Url> = {
            let mut updated = Vec::new();
            for mut r in self.ast_map.iter_mut() {
                let ast = r.value_mut();
                ast.update_globals_shared(Arc::clone(&globals));
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
    }

    pub(crate) async fn reload_flsproject(&self) -> Vec<crate::flsproject::FlsValidationError> {
        let in_memory = self.doc_map.iter().find_map(|r| {
            if crate::is_flsproject(r.key()) {
                Some((r.key().clone(), r.value().to_string()))
            } else {
                None
            }
        });

        let file_data = if let Some(mem) = in_memory {
            Some(mem)
        } else {
            let mut found = None;
            for r in self.workspace_map.iter() {
                if let Ok(path) = r.key().to_file_path() {
                    let fls_path = path.join("flsproject.fnl");
                    if fls_path.is_file()
                        && let Ok(text) = std::fs::read_to_string(&fls_path)
                        && let Ok(uri) = Url::from_file_path(&fls_path)
                    {
                        found = Some((uri, text));
                        break;
                    }
                }
            }
            found
        };

        let Some((fls_uri, text)) = file_data else {
            return Vec::new();
        };

        let (parsed_config, errors) = crate::flsproject::parse_and_validate_flsproject(&text);
        if let Ok(mut config) = self.config.write() {
            config.fls = parsed_config;
        }

        for err in &errors {
            self.client.show_message(MessageType::WARNING, &err.message).await;
        }

        if let Some(doc) = self.doc_map.get(&fls_uri) {
            let doc_snapshot = doc.clone();
            drop(doc);
            let ast = if let Some(ast) = self.ast_map.get(&fls_uri) {
                ast.clone()
            } else {
                let ast = fennel_parser::parse_shared(text.chars(), self.collect_globals_arc());
                self.ast_map.insert(fls_uri.clone(), ast.clone());
                ast
            };
            let diagnostics = self.compute_diagnostics(&doc_snapshot, &fls_uri, &ast, false);
            self.client.publish_diagnostics(fls_uri, diagnostics, None).await;
        }

        self.refresh_ast_globals_and_diagnostics().await;

        errors
    }

    pub(crate) async fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }

    pub(crate) async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client.log_message(MessageType::LOG, "file opened!").await;
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        let doc = ropey::Rope::from_str(&text);
        self.doc_map.insert(uri.clone(), doc.clone());
        self.open_docs.insert(uri.clone());

        let globals = self.collect_globals_arc();
        let ast = fennel_parser::parse_shared(text.chars(), globals);
        self.publish_diagnostics(&doc, uri.clone(), &ast, Some(version), true).await;

        self.on_save_or_open_errors.insert(uri.clone(), ast.on_save_errors().cloned().collect());

        self.ast_map.insert(uri.clone(), ast);

        if crate::is_flsproject(&uri) {
            self.reload_flsproject().await;
        }
    }

    pub(crate) async fn did_change(&self, params: DidChangeTextDocumentParams) {
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

        let globals = self.collect_globals_arc();
        let (doc_snapshot, ast) = {
            let ast = fennel_parser::parse_shared(doc.chars(), globals);
            (doc.clone(), ast)
        };
        // `doc` is a write guard on doc_map; release it before awaiting.
        drop(doc);

        let diagnostics = self.compute_diagnostics(&doc_snapshot, &uri, &ast, false);
        self.ast_map.insert(uri.clone(), ast);

        self.client.publish_diagnostics(uri.clone(), diagnostics, Some(version)).await;

        if crate::is_flsproject(&uri) {
            self.reload_flsproject().await;
        }
    }

    pub(crate) async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if crate::is_flsproject(&uri) {
            self.reload_flsproject().await;
            return;
        }
        let Some(ast) = self.ast_map.get(&uri) else {
            return;
        };
        let Some(doc) = self.doc_map.get(&uri) else {
            return;
        };
        // Compute with the guards alive, then release them before awaiting.
        let diagnostics = self.compute_diagnostics(&doc, &uri, &ast, true);
        drop(doc);
        drop(ast);
        self.client.publish_diagnostics(uri, diagnostics, None).await;
    }

    /// Re-index files that changed on disk.
    ///
    /// Without this, `ast_map` entries for workspace files are only ever written by
    /// `did_open`/`did_change`, so a file edited outside the editor keeps serving a
    /// stale AST to goto-definition, references, and rename.
    pub(crate) async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let mut fls_changed = false;
        for change in params.changes {
            if crate::is_flsproject(&change.uri) {
                fls_changed = true;
            }
            // Skip open buffers: the editor is the source of truth for those, and
            // re-reading from disk would clobber unsaved edits.
            //
            // `doc_map` cannot answer this on its own — background indexing stores
            // disk-read files there too — so `open_docs` tracks real editor buffers.
            if self.open_docs.contains(&change.uri) {
                continue;
            }

            match change.typ {
                FileChangeType::CREATED | FileChangeType::CHANGED => {
                    self.reindex_from_disk(&change.uri).await;
                }
                FileChangeType::DELETED => {
                    // Drop the cached entry so the file stops resolving at all.
                    self.ast_map.remove(&change.uri);
                    self.doc_map.remove(&change.uri);
                    self.on_save_or_open_errors.remove(&change.uri);
                }
                _ => {}
            }
        }
        if fls_changed {
            self.reload_flsproject().await;
        }
    }

    pub(crate) async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.free_doc(&uri);
    }

    pub(crate) async fn did_change_workspace_folders(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) {
        for r in &params.event.added {
            self.workspace_map.insert(r.uri.clone(), r.name.clone());
            self.index_workspace_folder(&r.uri).await;
        }
        for r in &params.event.removed {
            self.workspace_map.remove(&r.uri);
        }
        self.reload_flsproject().await;
    }

    pub(crate) async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let supports_config = self.supports_workspace_configuration();

        let is_empty = match &params.settings {
            serde_json::Value::Null => true,
            serde_json::Value::Object(map) => map.is_empty(),
            _ => false,
        };

        if supports_config && is_empty {
            self.pull_configuration().await;
            self.reload_flsproject().await;
            return;
        }

        match <config::Configuration as serde::Deserialize>::deserialize(params.settings) {
            Ok(config) => {
                *self.config.write().expect("config lock should not be poisoned") = config.clone();

                let libraries: Vec<Url> =
                    config.fennel.workspace.library.iter().map(|u| u.0.clone()).collect();
                for lib in libraries {
                    self.index_workspace_folder(&lib).await;
                }

                self.reload_flsproject().await;
                self.refresh_ast_globals_and_diagnostics().await;
            }
            Err(e) => {
                self.client.log_message(MessageType::ERROR, format!("Invalid config: {}", e)).await;
            }
        }
    }
}
