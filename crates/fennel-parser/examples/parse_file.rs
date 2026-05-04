use std::collections::HashSet;

fn main() {
    let file = std::env::args().nth(1).expect("expected file argument");
    let contents = std::fs::read_to_string(file).expect("failed to read file");
    let ast = fennel_parser::parse(contents.chars(), HashSet::new());
    ast.errors().for_each(|e| eprintln!("diagnostic: {:#?}", e))
}
