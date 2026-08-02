//! Read-only production surface for the frozen S1D lever contracts.

use std::process;

use optid::lever_contract::{contracts, validate_registry};

fn print_usage() {
    eprintln!("usage: optid-lever-contracts [--check|--list|--json]");
}

fn main() {
    let argument = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "--check".to_string());
    if let Err(error) = validate_registry() {
        eprintln!("optid-lever-contracts: invalid registry: {error:?}");
        process::exit(1);
    }

    match argument.as_str() {
        "--check" => println!("S1D lever contracts valid: {}", contracts().len()),
        "--list" => {
            for contract in contracts() {
                println!(
                    "{}\t{}\t{}",
                    contract.lever.as_str(),
                    contract.implementation.as_str(),
                    contract.credible_worst_case
                );
            }
        }
        "--json" => match serde_json::to_string_pretty(contracts()) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("optid-lever-contracts: cannot serialize registry: {error}");
                process::exit(1);
            }
        },
        "--help" | "-h" => print_usage(),
        _ => {
            print_usage();
            process::exit(2);
        }
    }
}
