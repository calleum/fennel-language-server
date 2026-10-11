//! Navigation: goto definition and find references.
//!
//! Part of the `tower_lsp::LanguageServer` impl for `Backend`, split out of
//! `main.rs` by feature area.

use std::collections::HashMap;

use fennel_parser::models;
use tower_lsp::{jsonrpc::Error, lsp_types::*};

use crate::{Backend, helper::*};

/// Inherent methods, grouped by feature area. The single
/// `LanguageServer` impl in `main.rs` forwards to these.
impl Backend {
    pub(crate) async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>, Error> {
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

    pub(crate) async fn references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>, Error> {
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
}
