//! Property-based tests for the language server.
//!
//! These live in-crate (rather than under `tests/`) because they need
//! `TestHarness`, which is `#[cfg(test)] pub(crate)`.
//!
//! The separator property is the one that earns its keep: an example test
//! covering only Unix paths passed while `module_name_from_relative` was broken
//! on Windows. Generating both separator styles is what surfaced it.

use proptest::prelude::*;

use crate::module_name_from_relative;

/// A path segment made of characters that are legal in Fennel module names.
fn segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}"
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Windows and Unix paths must produce the same module name.
    ///
    /// `Path::to_string_lossy` yields backslashes on Windows. The prefix and
    /// `/init` suffix rules were applied before separator normalization, so
    /// they silently never matched there — turning `fnl\foo\init.fnl` into
    /// `fnl.foo.init` instead of `foo`, and feeding wrong module names to
    /// auto-import.
    #[test]
    fn module_name_is_separator_independent(
        a in prop::collection::vec(segment(), 1..4),
        b in prop::collection::vec(segment(), 0..3),
        under_fnl in any::<bool>(),
    ) {
        let unix = {
            let mut parts = vec![];
            if under_fnl { parts.push("fnl".to_string()); }
            parts.extend(a.clone());
            if !b.is_empty() { parts.push("init".to_string()); }
            format!("{}.fnl", parts.join("/"))
        };
        let windows = unix.replace('/', "\\");

        let expected = module_name_from_relative(std::path::Path::new(&unix));
        let actual = module_name_from_relative(std::path::Path::new(&windows));

        // Built outside the assert: `proptest!` expands to a nested macro, so
        // inline format captures do not resolve inside it.
        let msg = format!("separator changed the module name: unix={unix:?} windows={windows:?}");
        prop_assert_eq!(actual, expected, "{}", msg);
    }

    /// A module name never contains a path separator.
    #[test]
    fn module_name_has_no_separators(a in prop::collection::vec(segment(), 1..4)) {
        let src = format!("fnl/{}.fnl", a.join("/"));
        if let Some(name) = module_name_from_relative(std::path::Path::new(&src)) {
            prop_assert!(!name.contains('/'), "separator survived in {name:?}");
            prop_assert!(!name.contains('\\'), "backslash survived in {name:?}");
        }
    }

}
