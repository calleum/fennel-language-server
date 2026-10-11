#[cfg(test)]
mod tests {
    use crate::tdd_helper::{STUBS_URI, setup_backend};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    /// LEVEL 0: The "What am I?" Hover
    /// Goal: When hovering, return the name of the SyntaxKind under the cursor.
    #[tokio::test]
    #[ignore = "hover provider not yet implemented"]
    async fn test_hover_syntax_kind() {
        let code = "(local x 123)";
        let (backend, uri) = setup_backend(code).await;

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(0, 10),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let response = backend.hover(params).await.expect("should not error");
        let hover = response.expect("should return hover content");

        if let HoverContents::Scalar(MarkedString::String(s)) = hover.contents {
            assert!(s.contains("NUMBER"), "Hover should contain 'NUMBER', but got: {}", s);
        } else {
            panic!("Expected simple string contents in hover");
        }
    }

    /// TDD for Task 3: SelectionRange Provider
    #[tokio::test]
    async fn test_selection_range() {
        let code = "(let [x 1] (+ x 2))";
        let (backend, uri) = setup_backend(code).await;

        let params = SelectionRangeParams {
            text_document: TextDocumentIdentifier::new(uri),
            positions: vec![Position::new(0, 14)],
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.selection_range(params).await.expect("should not error");
        let ranges = response.expect("should return selection ranges");
        assert_eq!(ranges.len(), 1);

        let range = &ranges[0];
        // Innermost: 'x'
        assert_eq!(range.range.start.character, 14);
        assert_eq!(range.range.end.character, 15);

        // Parent: (+ x 2)
        let parent = range.parent.as_ref().expect("should have parent range");
        assert_eq!(parent.range.start.character, 11);
        assert_eq!(parent.range.end.character, 18);

        // Grandparent: (let [x 1] (+ x 2))
        let grandparent = parent.parent.as_ref().expect("should have grandparent range");
        assert_eq!(grandparent.range.start.character, 0);
        assert_eq!(grandparent.range.end.character, 19);
    }

    #[tokio::test]
    async fn test_selection_range_table() {
        let code = "{:foo 123}";
        let (backend, uri) = setup_backend(code).await;

        let params = SelectionRangeParams {
            text_document: TextDocumentIdentifier::new(uri),
            positions: vec![Position::new(0, 6)],
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.selection_range(params).await.expect("should not error");
        let ranges = response.expect("should return selection ranges");
        assert_eq!(ranges.len(), 1);

        let range = &ranges[0];
        assert_eq!(range.range.start.character, 6);
        assert_eq!(range.range.end.character, 9);

        let parent = range.parent.as_ref().expect("should have parent range");
        assert!(parent.range.start.character <= 6);
        assert!(parent.range.end.character >= 9);
    }

    /// TDD for Task 6: Document Highlights
    #[tokio::test]
    async fn test_document_highlight() {
        let code = "(local x 1) (+ x x)";
        let (backend, uri) = setup_backend(code).await;

        // Position on 'x' definition
        let params = DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(0, 7),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.document_highlight(params).await.expect("should not error");
        let highlights = response.expect("should return highlights");

        // Should find 3 highlights: definition (Write) and two usages (Read)
        assert_eq!(highlights.len(), 3);

        let write_count =
            highlights.iter().filter(|h| h.kind == Some(DocumentHighlightKind::WRITE)).count();
        let read_count =
            highlights.iter().filter(|h| h.kind == Some(DocumentHighlightKind::READ)).count();

        assert_eq!(write_count, 1);
        assert_eq!(read_count, 2);
    }

    #[tokio::test]
    async fn test_document_highlight_from_usage() {
        let code = "(local x 1) (+ x x)";
        let (backend, uri) = setup_backend(code).await;

        let params = DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(0, 15),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.document_highlight(params).await.expect("should not error");
        let highlights = response.expect("should return highlights");
        assert_eq!(highlights.len(), 3);
        assert_eq!(
            highlights.iter().filter(|h| h.kind == Some(DocumentHighlightKind::WRITE)).count(),
            1
        );
        assert_eq!(
            highlights.iter().filter(|h| h.kind == Some(DocumentHighlightKind::READ)).count(),
            2
        );
    }

    /// TDD for Task 7: Signature Help
    #[tokio::test]
    async fn test_signature_help() {
        let code = "(fn my-func [a b c] nil) (my-func 1 )";
        let (backend, uri) = setup_backend(code).await;

        let params = SignatureHelpParams {
            context: None,
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(0, 36),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let response = backend.signature_help(params).await.expect("should not error");
        let help = response.expect("should return signature help");

        assert_eq!(help.signatures.len(), 1);
        let sig = &help.signatures[0];
        assert!(sig.label.contains("my-func"));
        assert!(sig.label.contains("[a b c]"));

        assert_eq!(help.active_parameter, Some(1));
    }

    #[tokio::test]
    async fn test_signature_help_stub() {
        let code = "(assert true )";
        let (backend, uri) = setup_backend(code).await;

        let params = SignatureHelpParams {
            context: None,
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(0, 13),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let response = backend.signature_help(params).await.expect("should not error");
        let help = response.expect("should return signature help");

        assert_eq!(help.signatures.len(), 1);
        let sig = &help.signatures[0];
        assert!(sig.label.contains("assert"));
        assert_eq!(help.active_parameter, Some(1));
    }

    /// TDD for Task 2: Folding Range Provider
    #[tokio::test]
    async fn test_folding_range() {
        let code = ";; comment line 1\n;; comment line 2\n(fn my-func [a\n             b]\n  (+ a\n     b))\n";
        let (backend, uri) = setup_backend(code).await;
        let params = FoldingRangeParams {
            text_document: TextDocumentIdentifier::new(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend.folding_range(params).await.expect("should not error");
        let folds = response.expect("should return folding ranges");
        assert!(!folds.is_empty());

        let comment_fold = folds.iter().find(|f| f.kind == Some(FoldingRangeKind::Comment));
        assert!(comment_fold.is_some(), "should have a comment fold");
        let cf = comment_fold.unwrap();
        assert_eq!(cf.start_line, 0);
        assert_eq!(cf.end_line, 1);

        let fn_fold = folds.iter().find(|f| f.start_line == 2 && f.end_line == 5);
        assert!(fn_fold.is_some(), "should have an outer function fold from line 2 to 5");

        let param_fold = folds.iter().find(|f| f.start_line == 2 && f.end_line == 3);
        assert!(param_fold.is_some(), "should have a param table fold from line 2 to 3");
    }

    /// TDD for Task 5: Completion detail and documentation
    #[tokio::test]
    async fn test_completion_detail_and_doc() {
        let code = r#"(fn add [x y] "adds x and y" (+ x y)) (local my-num 42) (add "#;
        let (backend, uri) = setup_backend(code).await;

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(0, code.len() as u32),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        let response = backend.completion(params).await.expect("should not error");
        let items = match response.expect("should return completion items") {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };

        let add_item = items.iter().find(|i| i.label == "add").expect("should have 'add'");
        assert_eq!(add_item.detail.as_deref(), Some("(fn add [x y])"));
        assert!(add_item.documentation.is_some());

        let num_item = items.iter().find(|i| i.label == "my-num").expect("should have 'my-num'");
        assert_eq!(num_item.detail.as_deref(), Some("number"));

        let assert_item = items.iter().find(|i| i.label == "assert").expect("should have 'assert'");
        assert!(assert_item.detail.as_ref().unwrap().contains("assert"));
        assert!(assert_item.documentation.is_some());
    }

    #[tokio::test]
    async fn test_workspace_symbols() {
        let code1 = "(local my-local 1)\n(fn compute-sum [a b] (+ a b))";
        let (backend, uri1) = setup_backend(code1).await;

        let code2 = "(global global-val 42)\n(macro helper-macro [x] x)";
        let uri2 = Url::parse("file:///other.fnl").expect("other URL should be valid");
        backend.doc_map.insert(uri2.clone(), ropey::Rope::from_str(code2));
        let ast2 = fennel_parser::parse(code2.chars(), std::collections::HashSet::new());
        backend.ast_map.insert(uri2.clone(), ast2);

        let params_all = WorkspaceSymbolParams {
            query: String::new(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let res_all = backend
            .symbol(params_all)
            .await
            .expect("workspace symbol should not error")
            .expect("should return symbols");
        let names: Vec<&str> = res_all.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"my-local"));
        assert!(names.contains(&"compute-sum"));
        assert!(names.contains(&"global-val"));
        assert!(names.contains(&"helper-macro"));

        let params_filtered = WorkspaceSymbolParams {
            query: "sum".into(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let res_filtered = backend
            .symbol(params_filtered)
            .await
            .expect("workspace symbol should not error")
            .expect("should return filtered symbols");
        assert_eq!(res_filtered.len(), 1);
        let first = res_filtered.first().expect("should have one symbol");
        assert_eq!(first.name, "compute-sum");
        assert_eq!(first.kind, SymbolKind::FUNCTION);
        assert_eq!(first.location.uri, uri1);
    }

    #[tokio::test]
    async fn test_stub_completion_and_goto_definition() {
        let code = "(string.format \"%s\" \"hi\") (table.insert [] 1)";
        let (backend, uri) = setup_backend(code).await;

        let def_params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(0, 8),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let def_resp = backend
            .goto_definition(def_params)
            .await
            .expect("should not error")
            .expect("should return definition");
        match def_resp {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.uri.as_str(), STUBS_URI);
            }
            GotoDefinitionResponse::Array(locs) => {
                let first = locs.first().expect("should have at least one location");
                assert_eq!(first.uri.as_str(), STUBS_URI);
            }
            GotoDefinitionResponse::Link(_) => panic!("unexpected Link response"),
        }

        let def_table_params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri.clone()),
                position: Position::new(0, 35),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let def_table_resp = backend
            .goto_definition(def_table_params)
            .await
            .expect("should not error")
            .expect("should return table.insert definition");
        match def_table_resp {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.uri.as_str(), STUBS_URI);
            }
            GotoDefinitionResponse::Array(locs) => {
                let first = locs.first().expect("should have at least one location");
                assert_eq!(first.uri.as_str(), STUBS_URI);
            }
            GotoDefinitionResponse::Link(_) => panic!("unexpected Link response"),
        }

        let comp_code = "(string.";
        let (backend_comp, uri_comp) = setup_backend(comp_code).await;
        let comp_params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri_comp),
                position: Position::new(0, 8),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(".".into()),
            }),
        };
        let comp_resp = backend_comp
            .completion(comp_params)
            .await
            .expect("should not error")
            .expect("should return completion items");
        let items = match comp_resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let format_item =
            items.iter().find(|i| i.label == "format").expect("should contain 'format'");
        assert!(format_item.detail.as_ref().unwrap().contains("string.format"));
        assert!(format_item.documentation.is_some());
    }

    #[tokio::test]
    async fn test_semantic_tokens() {
        let code = ";; my comment\n(fn add [x y] (+ x y))\n(local my-var 42)\n(print \"done\")";
        let (backend, uri) = setup_backend(code).await;

        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier::new(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = backend
            .semantic_tokens_full(params)
            .await
            .expect("should not error")
            .expect("should return semantic tokens");

        let tokens = match response {
            SemanticTokensResult::Tokens(t) => t.data,
            SemanticTokensResult::Partial(_) => panic!("unexpected partial result"),
        };

        let mut absolute_tokens = Vec::new();
        let mut current_line = 0;
        let mut current_col = 0;
        for t in &tokens {
            if t.delta_line > 0 {
                current_line += t.delta_line;
                current_col = t.delta_start;
            } else {
                current_col += t.delta_start;
            }
            absolute_tokens.push((
                current_line,
                current_col,
                t.length,
                t.token_type,
                t.token_modifiers_bitset,
            ));
        }

        use crate::semantic_tokens::*;

        let comment_tok = absolute_tokens.iter().find(|t| t.0 == 0);
        assert!(comment_tok.is_some());
        let c = comment_tok.unwrap();
        assert_eq!(c.2, 13);
        assert_eq!(c.3, TOKEN_TYPE_COMMENT);

        let fn_tok = absolute_tokens.iter().find(|t| t.0 == 1 && t.1 == 1);
        assert!(fn_tok.is_some());
        assert_eq!(fn_tok.unwrap().3, TOKEN_TYPE_KEYWORD);

        let add_tok = absolute_tokens.iter().find(|t| t.0 == 1 && t.1 == 4);
        assert!(add_tok.is_some());
        let a = add_tok.unwrap();
        assert_eq!(a.3, TOKEN_TYPE_FUNCTION);
        assert_ne!(a.4 & MODIFIER_DECLARATION, 0);

        let x_tok = absolute_tokens.iter().find(|t| t.0 == 1 && t.1 == 9);
        assert!(x_tok.is_some());
        let x = x_tok.unwrap();
        assert_eq!(x.3, TOKEN_TYPE_PARAMETER);
        assert_ne!(x.4 & MODIFIER_DECLARATION, 0);

        let plus_tok = absolute_tokens.iter().find(|t| t.0 == 1 && t.1 == 15);
        assert!(plus_tok.is_some());
        assert_eq!(plus_tok.unwrap().3, TOKEN_TYPE_OPERATOR);

        let x_use_tok = absolute_tokens.iter().find(|t| t.0 == 1 && t.1 == 17);
        assert!(x_use_tok.is_some());
        let xu = x_use_tok.unwrap();
        assert_eq!(xu.3, TOKEN_TYPE_PARAMETER);
        assert_eq!(xu.4 & MODIFIER_DECLARATION, 0);

        let var_tok = absolute_tokens.iter().find(|t| t.0 == 2 && t.1 == 7);
        assert!(var_tok.is_some());
        let v = var_tok.unwrap();
        assert_eq!(v.3, TOKEN_TYPE_VARIABLE);
        assert_ne!(v.4 & MODIFIER_DECLARATION, 0);

        let num_tok = absolute_tokens.iter().find(|t| t.0 == 2 && t.1 == 14);
        assert!(num_tok.is_some());
        assert_eq!(num_tok.unwrap().3, TOKEN_TYPE_NUMBER);

        let print_tok = absolute_tokens.iter().find(|t| t.0 == 3 && t.1 == 1);
        assert!(print_tok.is_some());
        let p = print_tok.unwrap();
        assert_eq!(p.3, TOKEN_TYPE_FUNCTION);
        assert_ne!(p.4 & MODIFIER_DEFAULT_LIBRARY, 0);

        let str_tok = absolute_tokens.iter().find(|t| t.0 == 3 && t.1 == 7);
        assert!(str_tok.is_some());
        assert_eq!(str_tok.unwrap().3, TOKEN_TYPE_STRING);
    }
}
