//! Randomized parser fuzzer.
//!
//! The deterministic corpus in `tests/robustness.rs` runs on every `cargo test`.
//! This binary runs a longer randomized search for when you have time to hunt:
//!
//! ```sh
//! cargo run --release -p fennel-parser --example fuzz -- 200000
//! ```
//!
//! It reuses the same invariants: never panic, always terminate, and never
//! emit an error range outside the source text.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// xorshift64 — deterministic and dependency-free, so a failure found here can
/// be reproduced by re-running with the same seed.
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
    "match",
];

fn generated_case(rng: &mut Rng) -> String {
    let n = 1 + rng.below(16);
    let mut out = String::new();
    for _ in 0..n {
        out.push_str(rng.pick(FRAGMENTS));
        if rng.below(3) == 0 {
            out.push(' ');
        }
    }
    out
}

fn main() {
    let iterations: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let seed: u64 =
        std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0x1234_5678_9abc_def0);

    eprintln!("fuzzing {iterations} cases (seed {seed:#x})");

    let mut rng = Rng::new(seed);
    let started = Instant::now();

    for i in 0..iterations {
        let case = generated_case(&mut rng);
        let text_len = case.len() as u32;

        let globals = HashSet::from(["print".to_string()]);
        let ast = fennel_parser::parse(case.chars(), globals);

        for error in ast.errors() {
            assert!(
                u32::from(error.range.start()) <= u32::from(error.range.end()),
                "inverted error range {:?} for input {case:?}",
                error.range
            );
            assert!(
                u32::from(error.range.end()) <= text_len,
                "error range {:?} escapes {text_len}-byte input {case:?}",
                error.range
            );
        }

        if i % 25_000 == 0 && i > 0 {
            eprintln!("  {i} cases ok ({:.1?})", started.elapsed());
        }
    }

    eprintln!("done: {iterations} cases in {:.2?} with no panics or bad ranges", started.elapsed());
    // Keep the budget tight: this is meant to catch pathological blowups, not
    // to enforce a performance target.
    assert!(started.elapsed() < Duration::from_secs(600), "fuzz run took too long");
}
