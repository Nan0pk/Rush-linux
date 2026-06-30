//! Quick smoke test: parses bench-list.toml and prints the menu.
use testos::BenchList;
use std::path::Path;

fn main() {
    let list = BenchList::load(Path::new("testos/bench-list.toml")).unwrap();
    println!("Loaded v{} with {} benches", list.version, list.benches.len());
    println!("Total ETA: {}", BenchList::format_duration(list.total_estimated_seconds()));
    println!();
    println!("Available benchmarks:");
    println!("  [0] Run all (estimated {})", BenchList::format_duration(list.total_estimated_seconds()));
    for (i, b) in list.benches.iter().enumerate() {
        let eta = BenchList::format_duration(b.estimated_seconds);
        let bat = if b.requires_battery { " [battery]" } else { "" };
        println!("  [{}] {} ({}){}", i + 1, b.name, eta, bat);
    }
}
