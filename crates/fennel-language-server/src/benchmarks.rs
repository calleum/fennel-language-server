use std::{collections::HashSet, path::PathBuf, time::Instant};

use tower_lsp::lsp_types::*;

use crate::{extract_symbol_completion_detail_and_doc, test_harness::TestHarness, view};

fn old_collect_globals(std_ast: &fennel_parser::Ast) -> HashSet<String> {
    let mut globals = HashSet::new();
    let (std_symbols, _, _, _) = std_ast.completion(std_ast.end_offset(), None);
    for sym in std_symbols {
        globals.insert(sym.token.text.clone());
    }
    globals
}

fn old_std_completions(std_ast: &fennel_parser::Ast) -> Vec<CompletionItem> {
    let (std_symbols, _, _, _) = std_ast.completion(std_ast.end_offset(), None);
    std_symbols
        .map(|symbol| {
            let (detail, documentation) = extract_symbol_completion_detail_and_doc(symbol, std_ast);
            CompletionItem {
                label: symbol.token.text.clone(),
                insert_text: Some(symbol.token.text.clone()),
                kind: Some(view::completion_scope_kind(symbol.scope.kind)),
                detail,
                documentation,
                ..Default::default()
            }
        })
        .collect()
}

#[tokio::test]
#[expect(
    clippy::cmp_owned,
    reason = "benchmarking old owned comparison against direct rope comparison"
)]
async fn test_benchmark_language_server_hot_paths() {
    let harness = TestHarness::with_temp_dir();
    let backend = &harness.backend;

    println!("\n=== Fennel Language Server Hot-Path Benchmarks ===\n");

    let (iters_globals, iters_comp, iters_rope, iters_find) =
        if cfg!(debug_assertions) { (20, 5, 200, 20) } else { (2000, 500, 20000, 2000) };

    let start = Instant::now();
    for _ in 0..iters_globals {
        let res = old_collect_globals(&backend.std_ast);
        std::hint::black_box(res);
    }
    let old_globals_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..iters_globals {
        let res = backend.collect_globals_arc();
        std::hint::black_box(res);
    }
    let new_globals_time = start.elapsed();

    let globals_speedup =
        old_globals_time.as_nanos() as f64 / new_globals_time.as_nanos().max(1) as f64;

    println!("--- Benchmark 1: Backend::collect_globals ({} iterations) ---", iters_globals);
    println!(
        "Dynamic std_ast traversal (old): {:>8.2?} (avg {:>6.2?} / iter)",
        old_globals_time,
        old_globals_time / iters_globals
    );
    println!(
        "Cached Arc clone (new):           {:>8.2?} (avg {:>6.2?} / iter)",
        new_globals_time,
        new_globals_time / iters_globals
    );
    println!("Speedup: {:.1}x\n", globals_speedup);

    let start = Instant::now();
    for _ in 0..iters_comp {
        let res = old_std_completions(&backend.std_ast);
        std::hint::black_box(res);
    }
    let old_comp_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..iters_comp {
        let res: Vec<CompletionItem> = backend.std_completions.iter().cloned().collect();
        std::hint::black_box(res);
    }
    let new_comp_time = start.elapsed();

    let comp_speedup = old_comp_time.as_nanos() as f64 / new_comp_time.as_nanos().max(1) as f64;

    println!("--- Benchmark 2: Autocomplete std items ({} iterations) ---", iters_comp);
    println!(
        "Dynamic parse + doc extraction:  {:>8.2?} (avg {:>6.2?} / iter)",
        old_comp_time,
        old_comp_time / iters_comp
    );
    println!(
        "Precomputed Vec clone:           {:>8.2?} (avg {:>6.2?} / iter)",
        new_comp_time,
        new_comp_time / iters_comp
    );
    println!("Speedup: {:.1}x\n", comp_speedup);

    let sample_file = include_str!("../../../stubs/lua54.fnl");
    let rope = ropey::Rope::from_str(sample_file);

    let start = Instant::now();
    for _ in 0..iters_rope {
        let matches = rope.to_string() == sample_file;
        std::hint::black_box(matches);
    }
    let old_rope_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..iters_rope {
        let matches = rope == sample_file;
        std::hint::black_box(matches);
    }
    let new_rope_time = start.elapsed();

    let rope_speedup = old_rope_time.as_nanos() as f64 / new_rope_time.as_nanos().max(1) as f64;

    println!("--- Benchmark 3: Document equality check ({} iterations, 14KB file) ---", iters_rope);
    println!(
        "Rope::to_string() allocation:    {:>8.2?} (avg {:>6.2?} / iter)",
        old_rope_time,
        old_rope_time / iters_rope
    );
    println!(
        "Direct zero-copy PartialEq:      {:>8.2?} (avg {:>6.2?} / iter)",
        new_rope_time,
        new_rope_time / iters_rope
    );
    println!("Speedup: {:.1}x\n", rope_speedup);

    let (_, base_url) =
        harness.write_file("src/main.fnl", "(local my-mod (require :util.helpers))");
    let _ = harness.write_file("src/util/helpers.fnl", "{:hello (fn [] (print :hi))}");

    let target = PathBuf::from("util.helpers");

    let start = Instant::now();
    for _ in 0..iters_find {
        let found = backend.find_file(&base_url, target.clone());
        std::hint::black_box(found);
    }
    let find_time = start.elapsed();

    let avg_find = find_time / iters_find;
    let ops_sec = iters_find as f64 / find_time.as_secs_f64();

    println!("--- Benchmark 4: Backend::find_file resolution ({} iterations) ---", iters_find);
    println!("Total time: {:>8.2?}", find_time);
    println!("Average lookup latency: {:>6.2?}", avg_find);
    println!("Throughput: {:.0} lookups/sec\n", ops_sec);

    if !cfg!(debug_assertions) {
        assert!(globals_speedup > 1.5, "collect_globals_arc should be at least 1.5x faster");
        assert!(comp_speedup > 1.5, "precomputed std_completions should be at least 1.5x faster");
        assert!(rope_speedup > 1.2, "zero-copy rope equality should be at least 1.2x faster");
    }
}
