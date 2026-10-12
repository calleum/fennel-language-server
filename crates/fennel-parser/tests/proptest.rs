//! Property-based tests.
//!
//! These assert invariants over generated input rather than fixed
//! `(input, output)` pairs. Where an example test covers one shape, these cover
//! the space and shrink a failure to the smallest input that still reproduces
//! it — which is how the separator bug in `module_name_from_relative` was found
//! (an example test using Unix paths passed on a codebase with a Windows-only
//! defect).
//!
//! Run the fast set on every `cargo test`. The heavier ones are gated behind
//! `--ignored` and run in a dedicated CI job.

use proptest::prelude::*;

// Fragments chosen to sit on the parser's error-recovery boundaries:
// unbalanced delimiters, truncated forms, and interleaved comments.
fn fragment() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        "(fn",
        "fn",
        "[a b]",
        "{: a b}",
        "local",
        "let",
        ":",
        "...",
        "->",
        "->>",
        "doto",
        "when",
        "collect",
        "while",
        "include",
        ":lua",
        "require",
        "import-macros",
        "macro",
        "varargs",
        "#",
        "@",
        "$",
        "x",
        "foo.bar",
        "a:b",
        "\"str\"",
        ":kw",
        "1",
        "1.5",
        "nil",
        "true",
        ";",
        "'",
        "`",
        "\\",
        " ",
        "\n",
        "\t",
        "--[[c]]",
        "-- line comment",
        "#{1}",
        "compile",
        "eval-compiler",
        "macros",
        "each",
        "pick-args",
    ])
}

// Arbitrary source text built from parser-relevant fragments.
fn source() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment(), 0..12).prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// The parser must never panic, whatever it is handed. A panic in an LSP
    /// server aborts the process and takes every other feature with it.
    #[test]
    fn parse_never_panics(src in source()) {
        let _ = fennel_parser::parse(src.chars(), Default::default());
    }

    /// Diagnostics must point inside the document. A range past the end would
    /// either mis-render or read out of bounds downstream.
    #[test]
    fn error_ranges_stay_in_bounds(src in source()) {
        let len = src.len() as u32;
        let ast = fennel_parser::parse(src.chars(), Default::default());
        for error in ast.errors() {
            let (start, end) = (u32::from(error.range.start()), u32::from(error.range.end()));
            prop_assert!(start <= end, "inverted range {:?} in {:?}", error.range, src);
            prop_assert!(end <= len, "range {:?} exceeds {} bytes in {:?}", error.range, len, src);
        }
    }

    /// Parsing is a pure function of its input; two passes must agree.
    #[test]
    fn parse_is_deterministic(src in source()) {
        let one = fennel_parser::parse(src.chars(), Default::default());
        let two = fennel_parser::parse(src.chars(), Default::default());
        let kinds = |a: &fennel_parser::Ast| {
            a.errors().map(|e| (e.range, format!("{:?}", e.kind))).collect::<Vec<_>>()
        };
        prop_assert_eq!(kinds(&one), kinds(&two), "non-deterministic parse of {:?}", src);
    }

    /// Whitespace and comments must not change which symbols are bound.
    #[test]
    fn definitions_ignore_interleaved_whitespace(
        body in "[a-z]{1,4}",
        pad in prop::sample::select(vec![" ", "\n", "\t", "  \n  "]),
    ) {
        let src = format!("(fn {body} [] 1)\n{{{body}}}\n");
        let padded = src.replace(' ', pad);
        let names = |s: &str| {
            let ast = fennel_parser::parse(s.chars(), Default::default());
            let mut n: Vec<String> =
                ast.all_l_symbols().map(|s| s.token.text.clone()).collect();
            n.sort();
            n
        };
        prop_assert_eq!(names(&src), names(&padded), "whitespace changed bindings");
    }
}

/// Heavier searches. These are `#[ignore]`d so the default `cargo test` stays
/// fast; CI runs them in a dedicated job after the main matrix passes.
mod slow {
    use super::*;

    // Pathological nesting must terminate quickly. This is the property
    // that caught the stack-overflow DoS: before `MAX_NESTING_DEPTH`, 8k
    // nested parens took ~5s in release and crashed beyond that.
    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

        #[test]
        #[ignore = "slow property test — run via the dedicated CI job"]
        fn deep_nesting_terminates(depth in 1usize..20_000) {
            let src = "(".repeat(depth);
            let ast = fennel_parser::parse(src.chars(), Default::default());
            // Over-deep input must be reported, not silently accepted.
            prop_assert!(ast.errors().next().is_some(), "accepted {depth} unclosed parens");
        }
    }
}
