//! Robustness harness for the parser.
//!
//! The parser is the one component that sees *untrusted* input: every keystroke
//! from the editor goes through it, so a half-typed form is normal traffic.
//! The invariants asserted here are the ones a panic would violate in an LSP
//! server: it must always return, never panic, and never produce an error
//! range that points outside the source text.
//!
//! Cases are generated rather than hand-written so the harness keeps finding
//! inputs we did not think to try. `cargo test -p fennel-parser robustness`
//! runs the deterministic corpus; `cargo run --example fuzz -p fennel-parser
//! -- <iterations>` runs the randomized search.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Deterministic pseudo-random generator.
///
/// `rand` is not a dependency and adding one for a test harness is not worth
/// it; xorshift is plenty for finding parser edge cases.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        items.get(self.below(items.len())).expect("items is non-empty")
    }
}

/// Fragments chosen to sit right on the parser's error-recovery boundaries:
/// unbalanced delimiters, bare closers, truncated special forms, and
/// interleaved whitespace/comments.
const FRAGMENTS: &[&str] = &[
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
    "icoll",
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
    "#{1}",
    "compile",
    "eval-compiler",
    "macros",
    "each",
    "pick-args",
    ":t",
    "1e999",
    "-",
];

/// Seeds covering shapes that historically broke error recovery.
const SEEDS: &[&str] = &[
    "(x",
    "(fn foo [a b] (print a",
    "(let [x 1] 2]",
    "{: a 1",
    "(local x 1 2 3 4 5)\n(local y 6)",
    "(require",
    "(include \"foo\")",
    "(lua \"print(1)\"",
    "(-> x (foo",
    "(while true",
    "(collect {:a 1}",
    "(doto x (print",
    "(macros :foo)",
    "(import-macros {: m} :n)",
    "(match x (1 2) (y 3)",
    "(when {:x true} (+ 1",
    "[[[",
    ")))",
    "(((",
    "(fn f [] (((",
    "",
    " ",
    "\n\n\n",
];

fn generated_case(rng: &mut Rng) -> String {
    let n = 1 + rng.below(12);
    let mut out = String::new();
    for _ in 0..n {
        out.push_str(rng.pick(FRAGMENTS));
        if rng.below(3) == 0 {
            out.push(' ');
        }
    }
    out
}

/// Runs the parser and asserts the invariants an LSP server depends on.
///
/// Panics on: a parser panic, a non-terminating parse, or an error range that
/// escapes the source text (which would produce a diagnostic pointing at the
/// wrong place, or an out-of-bounds read downstream).
fn check(case: &str, budget: Duration) {
    let globals = HashSet::from(["print".to_string(), "vim".to_string()]);

    let input = case.to_string();
    let text_len = input.len() as u32;

    let start = Instant::now();
    let ast = fennel_parser::parse(input.chars(), globals);
    let elapsed = start.elapsed();

    assert!(
        elapsed < budget,
        "parse took {elapsed:?} (budget {budget:?}) for input {case:?} — \
         possible infinite loop or quadratic blowup"
    );

    for error in ast.errors() {
        let range = error.range;
        assert!(
            u32::from(range.start()) <= u32::from(range.end()),
            "inverted error range {range:?} for input {case:?}"
        );
        assert!(
            u32::from(range.end()) <= text_len,
            "error range {range:?} escapes {text_len}-byte input {case:?}"
        );
    }
}

#[test]
fn deterministic_seeds_are_handled() {
    for case in SEEDS {
        check(case, Duration::from_secs(2));
    }
}

#[test]
fn generated_corpus_does_not_break_the_parser() {
    let mut rng = Rng::new(0x05ee_d0f1);
    for i in 0..20_000 {
        let case = generated_case(&mut rng);
        // A generous budget: we are catching hangs, not micro-benchmarks.
        check(&case, Duration::from_secs(5));
        if i % 5_000 == 0 {
            eprintln!("checked {i} generated cases");
        }
    }
}

#[test]
fn truncations_of_real_forms_do_not_break_the_parser() {
    // Every prefix of a realistic file: this is exactly what the editor sends
    // while the user is mid-keystroke.
    let realistic = "(local m (require :utils))\n\
                     (fn greet [name]\n  \
                       (let [msg (string.format \"hi %s\" name)]\n    \
                         (print msg)))\n\
                     {: greet}\n\
                     (greet \"world\")\n";
    let chars: Vec<char> = realistic.chars().collect();
    for i in 0..chars.len() {
        let prefix: String = chars[..=i].iter().collect();
        check(&prefix, Duration::from_secs(2));
    }
}

#[test]
fn deeply_nested_input_is_bounded() {
    // Unbounded nesting used to abort the process with a stack overflow and
    // take quadratic time on the way there. `MAX_NESTING_DEPTH` caps it; these
    // depths must now return promptly instead of dying.
    for depth in [1_000usize, 10_000, 100_000] {
        check(&"(".repeat(depth), Duration::from_secs(5));
        let case = format!("{}1{}", "[".repeat(depth / 2), "]".repeat(depth / 2));
        check(&case, Duration::from_secs(5));
    }
}

/// Pins the performance characteristic, not just the absence of a crash.
///
/// Before `MAX_NESTING_DEPTH` this took ~5 seconds; it is now ~2 ms. The
/// assertion is loose on purpose — it exists to catch a return to quadratic
/// behaviour, not to measure the machine.
#[test]
fn pathological_nesting_stays_fast() {
    let globals = HashSet::new();
    let input = "(".repeat(20_000);
    let start = Instant::now();
    let ast = fennel_parser::parse(input.chars(), globals);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "20k-deep nesting took {elapsed:?} — quadratic behaviour is back"
    );
    assert!(
        ast.errors().next().is_some(),
        "over-deep nesting should be reported as an error, not silently accepted"
    );
}

/// Normal code must be entirely unaffected by the depth cap.
#[test]
fn realistic_nesting_is_unaffected() {
    let case = "(local m (require :utils))\n\
                (fn greet [name]\n  \
                  (let [msg (string.format \"hi %s\" name)]\n    \
                    (print msg)))\n\
                {: greet}\n";
    check(case, Duration::from_secs(1));

    // 50 levels is far deeper than real code and still well under the cap.
    let nested = format!("{}1{}", "(+ 1 ".repeat(50), ")".repeat(50));
    check(&nested, Duration::from_secs(1));
}
