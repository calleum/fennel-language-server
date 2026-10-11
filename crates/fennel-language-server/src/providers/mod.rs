//! Feature-scoped slices of the `tower_lsp::LanguageServer` impl for `Backend`.
//!
//! Each submodule holds the providers for one area. They are split out of
//! `main.rs` purely for navigability — the trait impl itself still spans the
//! crate, since Rust permits an impl block to be continued across modules.

pub mod completion;
pub mod definition;
pub mod editing;
pub mod hierarchy;
pub mod hover;
pub mod lifecycle;
pub mod maintenance;
pub mod rename;
pub mod symbols;
// ---- Shared helpers -------------------------------------------------------
// These are pure functions used by more than one provider, so they live here
// rather than being duplicated per module.

use fennel_parser::{SyntaxKind, SyntaxNode, SyntaxToken};
use ropey::Rope;
use rowan::{TextRange, TextSize};
use tower_lsp::lsp_types::{CallHierarchyItem, SymbolKind, Url};

use crate::helper::*;

/// Returns the start offset of `name` inside the field token spanning `range`.
///
/// `RSymbol::token` carries the *full* dotted path in `text` (e.g. `math.sum`)
/// but only the *field* token in `range` (e.g. `.sum`), so the offset must be
/// derived from `range`, not from `text`. The field token always ends with the
/// segment name, which makes `range.end() - name.len()` the exact start in
/// bytes — correct for multi-byte identifiers too.
///
/// Returns `None` unless `symbol` genuinely ends with `name` as a segment
/// preceded by `.` (field) or `:` (method), which stops `foo.foobar` from
/// matching a rename of `bar`.
pub(crate) fn field_segment_offset(symbol: &str, range: TextRange, name: &str) -> Option<TextSize> {
    if name.is_empty() || name.len() > usize::from(range.len()) {
        return None;
    }
    let name_start = symbol.len().checked_sub(name.len())?;
    if !symbol.ends_with(name) {
        return None;
    }

    // The segment must be introduced by a field (`.`) or method (`:`)
    // separator, so `foobar` never matches a rename of `bar`.
    let prefix = symbol.get(..name_start)?;
    let last = prefix.chars().next_back()?;
    if last != '.' && last != ':' {
        return None;
    }

    Some(range.end() - TextSize::from(name.len() as u32))
}

pub(crate) fn fn_name_token(fn_node: &SyntaxNode) -> Option<SyntaxToken> {
    fn_node.children().find(|n| n.kind() == SyntaxKind::N_FN_NAME)?.first_token()
}

pub(crate) fn get_enclosing_fn_info(
    fn_node: &SyntaxNode,
    doc: &Rope,
    uri: &Url,
) -> Option<CallHierarchyItem> {
    let (name, name_token) = if let Some(token) = fn_name_token(fn_node) {
        (token.text().to_string(), token)
    } else {
        let parent = fn_node.parent()?;
        let binding_token =
            parent.children().find(|n| n.kind() == SyntaxKind::N_LOCAL)?.first_token()?;
        (binding_token.text().to_string(), binding_token)
    };

    let range = lsp_range(doc, fn_node.text_range()).ok()?;
    let selection_range = lsp_range(doc, name_token.text_range()).ok()?;

    Some(CallHierarchyItem {
        name,
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: uri.clone(),
        range,
        selection_range,
        data: None,
    })
}

pub(crate) fn is_selection_node_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::N_LIST
            | SyntaxKind::N_MACRO_LIST
            | SyntaxKind::N_SEQ_TABLE
            | SyntaxKind::N_KV_TABLE
            | SyntaxKind::N_ASSIGN_TABLE
            | SyntaxKind::N_FOR_TABLE
            | SyntaxKind::N_EACH_TABLE
            | SyntaxKind::N_PARAM_TABLE
            | SyntaxKind::N_ICOLLECT_TABLE
            | SyntaxKind::N_COLLECT_TABLE
            | SyntaxKind::N_FCOLLECT_TABLE
            | SyntaxKind::N_ACCUMULATE_TABLE
            | SyntaxKind::N_KV_PAIR
            | SyntaxKind::N_NV_PAIR
            | SyntaxKind::N_ASSIGN_PAIR
            | SyntaxKind::N_SET_PAIR
            | SyntaxKind::ROOT
    )
}

pub(crate) fn is_folding_node_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::N_LIST
            | SyntaxKind::N_MACRO_LIST
            | SyntaxKind::N_SEQ_TABLE
            | SyntaxKind::N_KV_TABLE
            | SyntaxKind::N_ASSIGN_TABLE
            | SyntaxKind::N_FOR_TABLE
            | SyntaxKind::N_EACH_TABLE
            | SyntaxKind::N_PARAM_TABLE
            | SyntaxKind::N_ICOLLECT_TABLE
            | SyntaxKind::N_COLLECT_TABLE
            | SyntaxKind::N_FCOLLECT_TABLE
            | SyntaxKind::N_ACCUMULATE_TABLE
            | SyntaxKind::N_FN
            | SyntaxKind::N_LAMBDA
            | SyntaxKind::N_MACRO
            | SyntaxKind::N_LET
            | SyntaxKind::N_MATCH
            | SyntaxKind::N_WHILE
    )
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use crate::test_harness::TestHarness;

    use rowan::TextRange;

    #[test]
    fn field_segment_offset_uses_range_not_text_length() {
        // `utils.add`: text is 9 bytes, the `.add` field token starts at byte 5
        // and spans 4 bytes. The edit must target `add`, i.e. byte 6.
        let range = TextRange::new(5.into(), 9.into());
        let offset = field_segment_offset("utils.add", range, "add")
            .expect("utils.add should yield an offset for add");
        assert_eq!(u32::from(offset), 6);
    }

    #[test]
    fn field_segment_offset_rejects_suffix_of_another_segment() {
        // `utils.foobar` must not match a rename of `bar`: the character before
        // `bar` is `o`, not a `.`/`:` separator.
        let range = TextRange::new(5.into(), 14.into());
        assert!(field_segment_offset("utils.foobar", range, "bar").is_none());
    }

    #[test]
    fn field_segment_offset_handles_multibyte_names() {
        // `utils.über`: the `.über` field token spans bytes 5..12. `über` is 5
        // bytes (`ü` is two), so it starts at 12 - 5 = 7, not at the char index
        // 6. The byte offset must stay exact.
        let range = TextRange::new(5.into(), 12.into());
        let offset = field_segment_offset("utils.über", range, "über")
            .expect("multi-byte name should match");
        assert_eq!(u32::from(offset), 7);
    }

    #[test]
    fn field_segment_offset_supports_method_syntax() {
        let range = TextRange::new(5.into(), 11.into());
        let offset = field_segment_offset("utils:adder", range, "adder")
            .expect("method syntax should match");
        assert_eq!(u32::from(offset), 6);
    }

    #[test]
    fn field_segment_offset_rejects_name_longer_than_field_token() {
        let range = TextRange::new(5.into(), 9.into());
        assert!(field_segment_offset("utils.add", range, "something-else").is_none());
        assert!(field_segment_offset("utils.add", range, "").is_none());
    }

    #[tokio::test]
    async fn uri_to_module_name_prefers_longest_matching_root() {
        let mut harness = TestHarness::with_temp_dir();
        harness.add_library_dir(&harness.temp_path().join("outer"));

        let inner_uri = harness.write_and_open_file("outer/inner/pkg.fnl", "(fn f [] 1)");
        let outer_uri = harness.write_and_open_file("outer/other.fnl", "(fn g [] 1)");

        // Both files sit under the same root here, so each maps by its own
        // relative path.
        assert_eq!(harness.backend.uri_to_module_name(&inner_uri).as_deref(), Some("inner.pkg"));
        assert_eq!(harness.backend.uri_to_module_name(&outer_uri).as_deref(), Some("other"));
    }
}
