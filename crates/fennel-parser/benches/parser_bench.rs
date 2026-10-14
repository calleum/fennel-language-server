use std::{collections::HashSet, sync::Arc, time::Instant};

use fennel_parser::{Ast, TextRange, models};

fn unused_l_symbols_naive(ast: &Ast) -> Vec<TextRange> {
    ast.all_l_symbols()
        .filter_map(|l_symbol| {
            if !l_symbol.token.text.starts_with('_')
                && l_symbol.scope.kind != models::ScopeKind::Global
                && l_symbol.token.text != "..."
                && !l_symbol.token.text.contains('.')
                && !ast.r_symbols().iter().any(|r_symbol| l_symbol.contains_token(&r_symbol.token))
            {
                Some(l_symbol.token.range)
            } else {
                None
            }
        })
        .collect()
}

fn generate_fennel_code(symbol_count: usize) -> String {
    let mut code = String::with_capacity(symbol_count * 60);
    for i in 0..symbol_count {
        code.push_str(&format!("(local var_{} {})\n", i, i));
        if i % 3 == 0 {
            code.push_str(&format!("(print var_{})\n", i));
        }
    }
    code
}

fn main() {
    println!("\n=== Fennel Parser Performance Benchmarks ===\n");

    let std_fnl = include_str!("../../../stubs/lua54.fnl");
    let globals_set: HashSet<String> = HashSet::new();
    let globals_arc: Arc<HashSet<String>> = Arc::new(HashSet::new());

    println!("--- Benchmark 1: Parsing std library (lua54.fnl, 100 iterations) ---");
    let parse_iters: u32 = 100;

    let start = Instant::now();
    for _ in 0..parse_iters {
        let _ = fennel_parser::parse(std_fnl.chars(), globals_set.clone());
    }
    let owned_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..parse_iters {
        let _ = fennel_parser::parse_shared(std_fnl.chars(), Arc::clone(&globals_arc));
    }
    let shared_time = start.elapsed();

    println!(
        "parse (owned HashSet):  {:>8.2?} (avg {:>6.2?} / iter)",
        owned_time,
        owned_time / parse_iters
    );
    println!(
        "parse_shared (Arc):     {:>8.2?} (avg {:>6.2?} / iter)",
        shared_time,
        shared_time / parse_iters
    );
    let parse_speedup = owned_time.as_nanos() as f64 / shared_time.as_nanos() as f64;
    println!("Speedup: {:.2}x\n", parse_speedup);

    println!("--- Benchmark 2: Symbol Resolution Scaling (O(L+R) vs O(L*R)) ---");
    println!(
        "{:<12} | {:<12} | {:<16} | {:<16} | {:<8}",
        "Symbols", "Iterations", "Naive O(L*R)", "Indexed O(L+R)", "Speedup"
    );
    println!("{:-<12}-|-{:-<12}-|-{:-<16}-|-{:-<16}-|-{:-<8}", "", "", "", "", "");

    let test_cases: [(usize, u32); 3] = [(50, 100), (200, 50), (600, 20)];

    for (symbols, iters) in test_cases {
        let code = generate_fennel_code(symbols);
        let ast = fennel_parser::parse_shared(code.chars(), Arc::clone(&globals_arc));

        let start = Instant::now();
        for _ in 0..iters {
            let res = unused_l_symbols_naive(&ast);
            std::hint::black_box(res);
        }
        let naive_duration = start.elapsed();

        let start = Instant::now();
        for _ in 0..iters {
            let res = ast.unused_l_symbol_errors.clone();
            std::hint::black_box(res);
        }
        let indexed_duration = start.elapsed();

        let naive_avg = naive_duration / iters;
        let indexed_avg = indexed_duration / iters;
        let speedup = naive_duration.as_nanos() as f64 / indexed_duration.as_nanos().max(1) as f64;

        println!(
            "{:<12} | {:<12} | {:>14.2?} | {:>14.2?} | {:>6.1}x",
            symbols, iters, naive_avg, indexed_avg, speedup
        );
    }

    println!("\nBenchmark complete.\n");
}
