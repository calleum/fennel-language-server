#[cfg(test)]
mod tests {
    use crate::tdd_helper::setup_backend;
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    /// LEVEL 0: The "What am I?" Hover
    /// Goal: When hovering, return the name of the SyntaxKind under the cursor.
    #[tokio::test]
    #[ignore = "hover provider not yet implemented"]
    async fn test_hover_syntax_kind() {
        let code = "(local x 123)";
        let (backend, uri) = setup_backend(code).await;

        // Position on '123'
        // (local x 123)
        // 01234567890
        //           ^ 10
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
    #[ignore = "selection_range provider not yet implemented"]
    async fn test_selection_range() {
        let code = "(let [x 1] (+ x 2))";
        let (backend, uri) = setup_backend(code).await;

        // Position on 'x' in (+ x 2)
        // (let [x 1] (+ x 2))
        // 0123456789012345678
        //               ^ 14
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

    /// TDD for Task 6: Document Highlights
    #[tokio::test]
    #[ignore = "document_highlight provider not yet implemented"]
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

    /// TDD for Task 7: Signature Help
    #[tokio::test]
    #[ignore = "signature_help provider not yet implemented"]
    async fn test_signature_help() {
        let code = "(fn my-func [a b c] nil) (my-func 1 )";
        let (backend, uri) = setup_backend(code).await;

        // Position after '1 '
        // (fn my-func [a b c] nil) (my-func 1 )
        // 0123456789012345678901234567890123456
        //                                     ^ 36
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

        // Active parameter should be 1 (the second one, 'b')
        assert_eq!(help.active_parameter, Some(1));
    }
}
