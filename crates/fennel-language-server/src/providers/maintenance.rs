//! Semantic tokens.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, semantic_tokens};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>, Error> {
        let uri = params.text_document.uri;
        let ast = self.ast_map.get(&uri).ok_or_else(Error::invalid_request)?;
        let doc = self.doc_map.get(&uri).ok_or_else(Error::invalid_request)?;

        let tokens = semantic_tokens::semantic_tokens_full(&doc, &ast, &self.std_ast);
        Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }
}
