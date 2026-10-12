#[cfg(test)]
mod tests {
    use crate::tdd_helper::{STUBS_URI, TestHarness, setup_backend};
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

    #[tokio::test]
    async fn test_prepare_rename() {
        let code = ";; a comment\n(let [foo 1] (+ foo 2))\n(print \"hello\")";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test.fnl", code);

        let res = harness.prepare_rename(&uri, 1, 6).await.unwrap();
        assert!(res.is_some());
        if let Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }) = res {
            assert_eq!(placeholder, "foo");
            assert_eq!(range.start.line, 1);
            assert_eq!(range.start.character, 6);
            assert_eq!(range.end.character, 9);
        } else {
            panic!("Expected RangeWithPlaceholder");
        }

        let res = harness.prepare_rename(&uri, 1, 16).await.unwrap();
        assert!(res.is_some());
        if let Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) = res {
            assert_eq!(placeholder, "foo");
        } else {
            panic!("Expected RangeWithPlaceholder");
        }

        let res = harness.prepare_rename(&uri, 0, 3).await.unwrap();
        assert!(res.is_none());

        let res = harness.prepare_rename(&uri, 1, 1).await.unwrap();
        assert!(res.is_none());

        let res = harness.prepare_rename(&uri, 1, 10).await.unwrap();
        assert!(res.is_none());

        let res = harness.prepare_rename(&uri, 2, 1).await.unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn test_cross_file_rename_module() {
        let utils_code = "(fn add [a b] (+ a b))\n{: add}";
        let main_code = "(local utils (require :utils))\n(utils.add 1 2)";

        let mut harness = TestHarness::with_temp_dir();
        let utils_uri = harness.write_and_open_file("utils.fnl", utils_code);
        let main_uri = harness.write_and_open_file("main.fnl", main_code);

        let edit = harness.rename(&utils_uri, 0, 4, "sum").await.unwrap();
        assert!(edit.is_some());
        let edit = edit.unwrap();
        let changes = edit.changes.expect("changes map should exist");

        assert!(changes.contains_key(&utils_uri), "should update utils.fnl");
        assert!(changes.contains_key(&main_uri), "should update main.fnl");

        let utils_edits = &changes[&utils_uri];
        assert_eq!(utils_edits.len(), 2);
        assert!(utils_edits.iter().all(|e| e.new_text == "sum"));

        let main_edits = &changes[&main_uri];
        assert_eq!(main_edits.len(), 1);
        assert_eq!(main_edits[0].new_text, "sum");
        assert_eq!(main_edits[0].range.start.line, 1);
        assert_eq!(main_edits[0].range.start.character, 7);
        assert_eq!(main_edits[0].range.end.character, 10);
    }

    #[tokio::test]
    async fn test_cross_file_rename_from_usage() {
        let utils_code = "(fn add [a b] (+ a b))\n{: add}";
        let main_code = "(local utils (require :utils))\n(utils.add 1 2)";

        let mut harness = TestHarness::with_temp_dir();
        let utils_uri = harness.write_and_open_file("utils.fnl", utils_code);
        let main_uri = harness.write_and_open_file("main.fnl", main_code);

        let edit = harness.rename(&main_uri, 1, 7, "sum").await.unwrap();
        assert!(edit.is_some());
        let edit = edit.unwrap();
        let changes = edit.changes.expect("changes map should exist");

        assert!(changes.contains_key(&utils_uri));
        assert!(changes.contains_key(&main_uri));
    }

    #[tokio::test]
    async fn test_cross_file_rename_macros() {
        let macro_code = "(fn my-macro [] (print \"macro\"))\n{: my-macro}";
        let main_code = "(import-macros {: my-macro} :mymacros)\n(my-macro)";

        let mut harness = TestHarness::with_temp_dir();
        let macro_uri = harness.write_and_open_file("mymacros.fnl", macro_code);
        let main_uri = harness.write_and_open_file("main.fnl", main_code);

        let edit = harness.rename(&macro_uri, 0, 4, "cool-macro").await.unwrap();
        assert!(edit.is_some());
        let edit = edit.unwrap();
        let changes = edit.changes.expect("changes map should exist");

        assert!(changes.contains_key(&macro_uri), "should update mymacros.fnl");
        assert!(changes.contains_key(&main_uri), "should update main.fnl");

        let macro_edits = &changes[&macro_uri];
        assert_eq!(macro_edits.len(), 2);

        let main_edits = &changes[&main_uri];
        assert_eq!(main_edits.len(), 2);
        assert!(main_edits.iter().all(|e| e.new_text == "cool-macro"));
    }

    #[tokio::test]
    async fn test_document_link() {
        let mut harness = TestHarness::with_temp_dir();
        let (_lib_path, lib_uri) = harness.write_file("mylib.fnl", "(fn foo [] 1)");
        let main_code = "(local lsp (require :mylib))\n(local other (require :missing))";
        let main_uri = harness.write_and_open_file("main.fnl", main_code);

        let links = harness.document_link(&main_uri).await.unwrap();
        assert!(links.is_some());
        let links = links.unwrap();
        assert_eq!(links.len(), 2);

        assert_eq!(links[0].target, Some(lib_uri));
        assert_eq!(links[0].range.start.line, 0);

        assert_eq!(links[1].target, None);
        assert_eq!(links[1].range.start.line, 1);
    }

    #[tokio::test]
    async fn test_while_document_symbol_and_folding() {
        let code = "(while condition\n  (let [x 1]\n    x))\n";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_while.fnl", code);

        let symbols_resp = harness.document_symbols(&uri).await.unwrap().unwrap();
        if let DocumentSymbolResponse::Nested(symbols) = symbols_resp {
            assert_eq!(symbols.len(), 1);
            assert_eq!(symbols[0].name, "while");
            assert_eq!(symbols[0].kind, SymbolKind::OPERATOR);
            let children = symbols[0].children.as_ref().unwrap();
            assert_eq!(children[0].name, "x");
        } else {
            panic!("Expected nested document symbols");
        }

        let folds = harness.folding_range(&uri).await.unwrap().unwrap();
        assert!(folds.iter().any(|f| f.start_line == 0 && f.end_line == 2));
    }

    #[tokio::test]
    async fn test_lua_hover() {
        let code = "(lua \"print('hello')\")";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_lua.fnl", code);

        let hover = harness.hover(&uri, 0, 8).await.unwrap();
        assert!(hover.is_some());
        let hover = hover.unwrap();
        if let HoverContents::Scalar(MarkedString::LanguageString(ls)) = hover.contents {
            assert_eq!(ls.language, "lua");
            assert_eq!(ls.value, "print('hello')");
        } else {
            panic!("Expected LanguageString in hover");
        }
    }

    #[tokio::test]
    async fn test_inlay_hints() {
        let code = "(fn add [first second] (+ first second))\n(add 10 20)\n(add first 30)\n";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_inlay.fnl", code);

        let hints = harness
            .inlay_hint(&uri, Range::new(Position::new(0, 0), Position::new(3, 0)))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(hints.len(), 3);
        let h0 = hints.first().expect("hint 0");
        match &h0.label {
            InlayHintLabel::String(s) => assert_eq!(s, "first:"),
            _ => panic!("Expected String label"),
        }
        assert_eq!(h0.position, Position::new(1, 5));

        let h1 = hints.get(1).expect("hint 1");
        match &h1.label {
            InlayHintLabel::String(s) => assert_eq!(s, "second:"),
            _ => panic!("Expected String label"),
        }
        assert_eq!(h1.position, Position::new(1, 8));

        let h2 = hints.get(2).expect("hint 2");
        match &h2.label {
            InlayHintLabel::String(s) => assert_eq!(s, "second:"),
            _ => panic!("Expected String label"),
        }
        assert_eq!(h2.position, Position::new(2, 11));
    }

    #[tokio::test]
    async fn test_code_action_prefix_unused() {
        let code = "(fn foo [unused-param used-param]\n  used-param)\n";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_action_unused.fnl", code);

        let actions = harness
            .code_action(&uri, Range::new(Position::new(0, 9), Position::new(0, 21)), vec![])
            .await
            .unwrap()
            .unwrap();

        let prefix_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Prefix with _" => Some(ca),
            _ => None,
        });

        assert!(prefix_action.is_some(), "Expected 'Prefix with _' code action");
        let ca = prefix_action.unwrap();
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));

        let edit = ca.edit.as_ref().expect("expected workspace edit");
        let text_edits = edit.changes.as_ref().unwrap().get(&uri).unwrap();
        assert_eq!(text_edits.len(), 1);
        let first_edit = text_edits.first().unwrap();
        assert_eq!(first_edit.new_text, "_unused-param");
    }

    #[tokio::test]
    async fn test_code_action_add_global() {
        let code = "(print unknown_global_var)\n";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_action_global.fnl", code);

        let actions = harness
            .code_action(&uri, Range::new(Position::new(0, 7), Position::new(0, 25)), vec![])
            .await
            .unwrap()
            .unwrap();

        let global_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Add to globals configuration" => {
                Some(ca)
            }
            _ => None,
        });

        assert!(global_action.is_some(), "Expected 'Add to globals configuration' code action");
        let ca = global_action.unwrap();
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));

        let cmd = ca.command.as_ref().expect("expected command");
        assert_eq!(cmd.command, "fennel.addGlobal");
        let arg = cmd.arguments.as_ref().unwrap().first().unwrap();
        assert_eq!(arg.as_str().unwrap(), "unknown_global_var");

        let res =
            harness.execute_command(&cmd.command, cmd.arguments.clone().unwrap()).await.unwrap();
        assert_eq!(res, Some(serde_json::Value::Bool(true)));
    }

    #[tokio::test]
    async fn test_background_workspace_indexing() {
        let mut harness = TestHarness::with_temp_dir();
        let (_lib_path, lib_url) =
            harness.write_file("src/lib.fnl", "(fn compute-sum [a b] (+ a b))\n{: compute-sum}\n");
        let (_sub_path, sub_url) =
            harness.write_file("nested/deep/sub.fnl", "(fn deep-helper [] 123)\n{: deep-helper}\n");

        assert!(harness.backend.ast_map.get(&lib_url).is_none());
        assert!(harness.backend.ast_map.get(&sub_url).is_none());

        harness.initialize().await.unwrap();

        assert!(harness.backend.ast_map.get(&lib_url).is_some());
        assert!(harness.backend.doc_map.get(&lib_url).is_some());
        assert!(harness.backend.ast_map.get(&sub_url).is_some());
        assert!(harness.backend.doc_map.get(&sub_url).is_some());

        let symbols = harness.workspace_symbols("compute-sum").await.unwrap().unwrap();
        assert!(!symbols.is_empty());
        assert_eq!(symbols.first().unwrap().name, "compute-sum");

        let deep_symbols = harness.workspace_symbols("deep-helper").await.unwrap().unwrap();
        assert!(!deep_symbols.is_empty());
        assert_eq!(deep_symbols.first().unwrap().name, "deep-helper");

        let main_url = harness.write_and_open_file(
            "src/main.fnl",
            "(local my-lib (require :lib))\n(my-lib.compute-sum 1 2)\n",
        );

        let def = harness.goto_definition(&main_url, 1, 8).await.unwrap();
        assert!(def.is_some());
        match def.unwrap() {
            GotoDefinitionResponse::Array(locs) => {
                assert!(locs.iter().any(|l| l.uri == lib_url));
            }
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.uri, lib_url);
            }
            _ => panic!("Expected definition response"),
        }
    }

    /// A workspace file edited outside the editor must not keep serving a
    /// stale AST. `did_change_watched_files` re-reads from disk.
    #[tokio::test]
    async fn test_watched_files_reindex_changed_file() {
        let harness = TestHarness::with_temp_dir();
        let (lib_path, lib_url) =
            harness.write_file("src/lib.fnl", "(fn original-name [] 1)\n{: original-name}\n");

        harness.initialize().await.unwrap();
        assert!(harness.backend.ast_map.get(&lib_url).is_some());

        // Edit on disk without opening the buffer.
        std::fs::write(&lib_path, "(fn renamed-fn [] 1)\n{: renamed-fn}\n").unwrap();

        harness
            .backend
            .did_change_watched_files(DidChangeWatchedFilesParams {
                changes: vec![FileEvent { uri: lib_url.clone(), typ: FileChangeType::CHANGED }],
            })
            .await;

        let ast = harness.backend.ast_map.get(&lib_url).expect("AST should still be cached");
        let exported: Vec<String> = ast.all_l_symbols().map(|s| s.token.text.clone()).collect();
        assert!(
            exported.contains(&"renamed-fn".to_string()),
            "stale AST: expected renamed-fn, got {exported:?}"
        );
        assert!(
            !exported.contains(&"original-name".to_string()),
            "stale symbol still present: {exported:?}"
        );
    }

    /// An unchanged file must not be needlessly re-parsed or republished.
    #[tokio::test]
    async fn test_reindex_from_disk_is_noop_when_unchanged() {
        let harness = TestHarness::with_temp_dir();
        let (_path, lib_url) = harness.write_file("src/lib.fnl", "(fn f [] 1)\n");

        harness.initialize().await.unwrap();

        let changed = harness.backend.reindex_from_disk(&lib_url).await;
        assert!(!changed, "unchanged file should report no change");
    }

    /// A deleted file must be evicted, not left behind as a stale entry.
    #[tokio::test]
    async fn test_reindex_from_disk_evicts_deleted_file() {
        let harness = TestHarness::with_temp_dir();
        let (path, lib_url) = harness.write_file("src/gone.fnl", "(fn f [] 1)\n");

        harness.initialize().await.unwrap();
        assert!(harness.backend.ast_map.get(&lib_url).is_some());

        std::fs::remove_file(&path).unwrap();

        let changed = harness.backend.reindex_from_disk(&lib_url).await;
        assert!(changed, "deleting a cached file should report a change");
        assert!(
            harness.backend.ast_map.get(&lib_url).is_none(),
            "deleted file should be evicted from the AST cache"
        );
        assert!(harness.backend.doc_map.get(&lib_url).is_none());
    }

    /// Open buffers are the editor's source of truth; a disk notification must
    /// never clobber unsaved edits by re-reading the file.
    #[tokio::test]
    async fn test_watched_files_skips_open_buffers() {
        let mut harness = TestHarness::with_temp_dir();
        let (path, lib_url) = harness.write_file("src/lib.fnl", "(fn on-disk [] 1)\n");

        // Open with buffer content that differs from disk (unsaved edits).
        let open_url = harness.write_and_open_file("src/lib.fnl", "(fn unsaved-edit [] 2)\n");
        assert_eq!(open_url, lib_url);

        // Now make disk disagree with the open buffer, as an external
        // `git checkout` or formatter would.
        std::fs::write(&path, "(fn written-by-someone-else [] 3)\n").unwrap();

        harness
            .backend
            .did_change_watched_files(DidChangeWatchedFilesParams {
                changes: vec![FileEvent { uri: lib_url.clone(), typ: FileChangeType::CHANGED }],
            })
            .await;

        let cached = harness.backend.doc_map.get(&lib_url).expect("buffer should stay cached");
        assert_eq!(
            cached.to_string(),
            "(fn unsaved-edit [] 2)\n",
            "unsaved buffer was clobbered by a disk re-read"
        );

        // And the on-disk file really did differ from the buffer.
        assert_ne!(std::fs::read_to_string(&path).unwrap(), cached.to_string());
    }

    /// `DELETED` events must evict the cached entry, not leave a stale AST
    /// that still resolves.
    #[tokio::test]
    async fn test_watched_files_delete_evicts_entry() {
        let harness = TestHarness::with_temp_dir();
        let (path, lib_url) = harness.write_file("src/lib.fnl", "(fn f [] 1)\n");

        harness.initialize().await.unwrap();
        assert!(harness.backend.ast_map.get(&lib_url).is_some());

        std::fs::remove_file(&path).unwrap();

        harness
            .backend
            .did_change_watched_files(DidChangeWatchedFilesParams {
                changes: vec![FileEvent { uri: lib_url.clone(), typ: FileChangeType::DELETED }],
            })
            .await;

        assert!(harness.backend.ast_map.get(&lib_url).is_none());
        assert!(harness.backend.doc_map.get(&lib_url).is_none());
    }

    #[tokio::test]
    async fn test_formatting_rewrites_document() {
        let mut harness = TestHarness::with_temp_dir();
        let uri = harness.write_and_open_file("main.fnl", "(local   x    1)\n(print x)");

        let edits = harness.formatting(&uri).await.unwrap().expect("should offer edits");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "(local x 1)\n(print x)\n");
    }

    /// Already-formatted code must produce no edits, so format-on-save does
    /// not churn the buffer.
    #[tokio::test]
    async fn test_formatting_is_noop_when_already_formatted() {
        let mut harness = TestHarness::with_temp_dir();
        let uri = harness.write_and_open_file("main.fnl", "(local x 1)\n");

        let edits = harness.formatting(&uri).await.unwrap().expect("should return a result");
        assert!(edits.is_empty(), "expected no edits, got {edits:?}");
    }

    /// Unparseable input must be declined, not reformatted.
    #[tokio::test]
    async fn test_formatting_declines_broken_input() {
        let mut harness = TestHarness::with_temp_dir();
        let uri = harness.write_and_open_file("main.fnl", "(local   x");

        let edits = harness.formatting(&uri).await.unwrap();
        assert!(edits.is_none(), "broken input should not be formatted, got {edits:?}");
    }

    #[tokio::test]
    async fn test_auto_import_completion() {
        let mut harness = TestHarness::with_temp_dir();
        let (_math_path, _math_url) =
            harness.write_file("src/math.fnl", "(fn add-numbers [a b] (+ a b))\n{: add-numbers}\n");
        let main_url = harness.write_and_open_file("src/main.fnl", "(print ");

        harness.initialize().await.unwrap();

        let resp = harness.completion(&main_url, 0, 7).await.unwrap().unwrap();
        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };

        let auto_import_item = items.iter().find(|i| i.label == "add-numbers");
        assert!(auto_import_item.is_some(), "Expected auto-import item for add-numbers");
        let item = auto_import_item.unwrap();

        assert_eq!(item.insert_text, Some("add-numbers".to_string()));
        assert!(item.detail.as_ref().unwrap().contains("auto-import"));

        let edits = item.additional_text_edits.as_ref().expect("expected additional text edits");
        assert_eq!(edits.len(), 1);
        let edit = edits.first().unwrap();
        assert_eq!(edit.range, Range::new(Position::new(0, 0), Position::new(0, 0)));
        assert_eq!(edit.new_text, "(local add-numbers (require :src.math))\n");
    }

    #[tokio::test]
    async fn test_call_hierarchy() {
        let code = "(fn callee [x]\n  (+ x 1))\n\n(fn middle []\n  (callee 10)\n  (callee 20))\n\n(fn caller []\n  (middle))\n";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_call_hierarchy.fnl", code);

        let items = harness.prepare_call_hierarchy(&uri, 3, 5).await.unwrap().unwrap();
        assert_eq!(items.len(), 1);
        let middle_item = items.first().unwrap().clone();
        assert_eq!(middle_item.name, "middle");
        assert_eq!(middle_item.kind, SymbolKind::FUNCTION);

        let incoming = harness.incoming_calls(middle_item.clone()).await.unwrap().unwrap();
        assert_eq!(incoming.len(), 1);
        let caller_inc = incoming.first().unwrap();
        assert_eq!(caller_inc.from.name, "caller");
        assert_eq!(caller_inc.from_ranges.len(), 1);
        assert_eq!(caller_inc.from_ranges.first().unwrap().start.line, 8);

        let outgoing = harness.outgoing_calls(middle_item).await.unwrap().unwrap();
        assert_eq!(outgoing.len(), 1);
        let callee_out = outgoing.first().unwrap();
        assert_eq!(callee_out.to.name, "callee");
        assert_eq!(callee_out.from_ranges.len(), 2);
        assert_eq!(callee_out.from_ranges.first().unwrap().start.line, 4);
        assert_eq!(callee_out.from_ranges.get(1).expect("call 2").start.line, 5);
    }

    #[tokio::test]
    async fn test_macro_expansion_hover() {
        let code = "(-> x (foo))";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_macro.fnl", code);

        let hover = harness.hover(&uri, 0, 1).await.unwrap().expect("hover should exist");
        if let HoverContents::Array(contents) = hover.contents {
            let combined = contents
                .iter()
                .map(|c| match c {
                    MarkedString::String(s) => s.clone(),
                    MarkedString::LanguageString(ls) => ls.value.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                combined.contains("(foo x)"),
                "Hover content should include expanded '(foo x)', got:\n{combined}"
            );
            assert!(
                combined.contains("Macro expands to:"),
                "Hover content should indicate macro expansion, got:\n{combined}"
            );
        } else {
            panic!("Expected HoverContents::Array, got {:?}", hover.contents);
        }

        let code_thread = "(-> x (f 1) (g 2))";
        let uri_thread = harness.open_virtual("file:///test_thread.fnl", code_thread);
        let hover_thread =
            harness.hover(&uri_thread, 0, 1).await.unwrap().expect("hover should exist");
        if let HoverContents::Array(contents) = hover_thread.contents {
            let combined = contents
                .iter()
                .map(|c| match c {
                    MarkedString::String(s) => s.clone(),
                    MarkedString::LanguageString(ls) => ls.value.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                combined.contains("(g (f x 1) 2)"),
                "Hover content should include expanded '(g (f x 1) 2)', got:\n{combined}"
            );
        } else {
            panic!("Expected HoverContents::Array");
        }
    }

    #[tokio::test]
    async fn test_macro_expansion_command_and_code_action() {
        let code = "(-> x (foo))";
        let mut harness = TestHarness::new();
        let uri = harness.open_virtual("file:///test_macro_cmd.fnl", code);

        let res = harness
            .execute_command(
                "fennel.expandMacro",
                vec![
                    serde_json::Value::String(uri.to_string()),
                    serde_json::to_value(Position::new(0, 1)).unwrap(),
                ],
            )
            .await
            .unwrap()
            .expect("should return macro expansion");
        assert_eq!(res, serde_json::Value::String("(foo x)".to_string()));

        let actions = harness
            .code_action(&uri, Range::new(Position::new(0, 1), Position::new(0, 3)), vec![])
            .await
            .unwrap()
            .expect("code actions");
        let expand_action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title == "Expand macro" => Some(ca),
                _ => None,
            })
            .expect("should include 'Expand macro' code action");

        let edit = expand_action.edit.as_ref().expect("edit should exist");
        let changes = edit.changes.as_ref().expect("changes should exist");
        let edits = changes.get(&uri).expect("edits for uri");
        let first_edit = edits.first().expect("edit should exist");
        assert_eq!(first_edit.new_text, "(foo x)");
    }
}
