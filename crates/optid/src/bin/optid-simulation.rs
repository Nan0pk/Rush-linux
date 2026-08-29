//! Feature-gated production CLI entry point for deterministic optid scenarios.

#[path = "../simulation.rs"]
mod simulation;

use std::path::PathBuf;

fn main() {
    let root = match parse_simulation_root(std::env::args().skip(1)) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("optid-simulation: {error}");
            print_usage();
            std::process::exit(2);
        }
    };

    match simulation::run_from_root(&root) {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("optid-simulation: could not serialize report: {error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("optid-simulation: simulation rejected: {error}");
            std::process::exit(1);
        }
    }
}

fn parse_simulation_root(args: impl IntoIterator<Item = String>) -> Result<PathBuf, String> {
    let mut root = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let value = if arg == "--simulation-root" {
            iter.next()
                .ok_or_else(|| "--simulation-root requires a path".to_string())?
        } else if let Some(value) = arg.strip_prefix("--simulation-root=") {
            if value.is_empty() {
                return Err("--simulation-root requires a path".to_string());
            }
            value.to_string()
        } else {
            return Err(format!(
                "--simulation-root cannot be combined with other arguments: {arg}"
            ));
        };
        if root.replace(PathBuf::from(value)).is_some() {
            return Err("--simulation-root may be supplied only once".to_string());
        }
    }
    root.ok_or_else(|| "--simulation-root is required".to_string())
}

fn print_usage() {
    eprintln!("Usage: optid-simulation --simulation-root PATH");
}

#[cfg(test)]
mod tests {
    use super::parse_simulation_root;
    use std::path::PathBuf;

    #[test]
    fn simulation_root_is_mandatory_and_exclusive() {
        assert_eq!(
            parse_simulation_root(["--simulation-root=/fixture".to_string()]),
            Ok(PathBuf::from("/fixture"))
        );
        assert!(parse_simulation_root(Vec::new()).is_err());
        assert!(parse_simulation_root([
            "--simulation-root".to_string(),
            "/fixture".to_string(),
            "--apply".to_string(),
        ])
        .is_err());
    }
}
