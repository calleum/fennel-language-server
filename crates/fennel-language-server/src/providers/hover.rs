//! Hover provider.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use fennel_parser::{SyntaxNode, models};
use rowan::{TextSize, ast::AstNode};
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, helper::*, view};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn hover(&self, params: HoverParams) -> Result<Option<Hover>, Error> {
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

                if let Some(macro_info) = ast.expand_macro(offset) {
                    let range = lsp_range(&doc, macro_info.call_range)?;
                    let header = MarkedString::LanguageString(LanguageString {
                        language: "fennel".into(),
                        value: macro_info.signature.to_string(),
                    });
                    let body = format!(
                        "{}\n\n---\n**Macro expands to:**\n```fennel\n{}\n```",
                        macro_info.docstring, macro_info.expansion
                    );
                    let contents = HoverContents::Array(vec![
                        header,
                        MarkedString::LanguageString(LanguageString {
                            language: "markdown".into(),
                            value: body,
                        }),
                    ]);
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
}
