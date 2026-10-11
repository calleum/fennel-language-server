//! Document and workspace lifecycle notifications.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use ropey::Rope;
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, config, helper::*};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn initialized(&self, _: InitializedParams) {
        self.client.log_message(MessageType::LOG, "initialized!").await;
        self.register_file_watchers().await;
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

        let globals = self.collect_globals();
        let ast = fennel_parser::parse(text.chars(), globals);
        self.publish_diagnostics(&doc, uri.clone(), &ast, Some(version), true).await;

        self.on_save_or_open_errors.insert(uri.clone(), ast.on_save_errors().cloned().collect());

        self.ast_map.insert(uri, ast);
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

        let globals = self.collect_globals();
        let (doc_snapshot, ast) = {
            let ast = fennel_parser::parse(doc.chars(), globals);
            (doc.clone(), ast)
        };
        // `doc` is a write guard on doc_map; release it before awaiting.
        drop(doc);

        let diagnostics = self.compute_diagnostics(&doc_snapshot, &uri, &ast, false);
        self.ast_map.insert(uri.clone(), ast);

        self.client.publish_diagnostics(uri, diagnostics, Some(version)).await;
    }

    pub(crate) async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
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
        for change in params.changes {
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
    }

    pub(crate) async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        match <config::Configuration as serde::Deserialize>::deserialize(params.settings) {
            Ok(config) => {
                *self.config.write().expect("config lock should not be poisoned") = config.clone();

                let libraries: Vec<Url> =
                    config.fennel.workspace.library.iter().map(|u| u.0.clone()).collect();
                for lib in libraries {
                    self.index_workspace_folder(&lib).await;
                }

                let globals = self.collect_globals();

                // Update every AST first and collect the URIs, then publish
                // diagnostics. `ast_map.iter_mut()` pins write guards on all
                // shards, and awaiting inside that loop would block every
                // concurrent request that reads `ast_map`.
                let updated: Vec<Url> = {
                    let mut updated = Vec::new();
                    for mut r in self.ast_map.iter_mut() {
                        let ast = r.value_mut();
                        ast.update_globals(globals.iter().cloned().collect());
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
                    // Compute while the read guards are alive, then drop them
                    // before awaiting the client.
                    let diagnostics = self.compute_diagnostics(&doc, &uri, &ast, false);
                    drop(doc);
                    drop(ast);
                    self.client.publish_diagnostics(uri, diagnostics, None).await;
                }
            }
            Err(e) => {
                self.client.log_message(MessageType::ERROR, format!("Invalid config: {}", e)).await;
            }
        }
    }
}
