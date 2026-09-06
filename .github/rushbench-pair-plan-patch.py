from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}")
    target.write_text(text.replace(old, new))


main = "crates/rushbench/src/main.rs"

replace_once(
    main,
    '''pub mod energy;
pub mod preset;
''',
    '''pub mod energy;
pub mod pair_plan;
pub mod preset;
''',
)

replace_once(
    main,
    '''    println!("  rushbench run preset=mixed-load-001 --tag=<lever>-<hostname> [--cycles <n>] [--out <dir>] [--ac-ok]");
    println!("  rushbench matrix [--ac-ok]");
''',
    '''    println!("  rushbench run preset=mixed-load-001 --tag=<lever>-<hostname> [--cycles <n>] [--out <dir>] [--ac-ok]");
    println!("  rushbench pair-plan --pairs <n> --seed <u64> [--out <file>]");
    println!("  rushbench matrix [--ac-ok]");
''',
)

anchor = '''        "matrix" => {
'''
pair_arm = '''        "pair-plan" => {
            let mut pairs: Option<usize> = None;
            let mut seed: Option<u64> = None;
            let mut out: Option<std::path::PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--pairs" => {
                        let value = args.get(i + 1).unwrap_or_else(|| {
                            eprintln!("Error: --pairs requires an argument");
                            std::process::exit(1);
                        });
                        pairs = match value.parse::<usize>() {
                            Ok(value) if value > 0 => Some(value),
                            _ => {
                                eprintln!("Error: --pairs must be at least 1");
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    }
                    "--seed" => {
                        let value = args.get(i + 1).unwrap_or_else(|| {
                            eprintln!("Error: --seed requires an argument");
                            std::process::exit(1);
                        });
                        seed = match value.parse::<u64>() {
                            Ok(value) => Some(value),
                            Err(_) => {
                                eprintln!("Error: --seed must be an unsigned integer");
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    }
                    "--out" => {
                        let value = args.get(i + 1).unwrap_or_else(|| {
                            eprintln!("Error: --out requires an argument");
                            std::process::exit(1);
                        });
                        out = Some(std::path::PathBuf::from(value));
                        i += 2;
                    }
                    unknown => {
                        eprintln!("Error: unknown pair-plan argument {unknown}");
                        print_usage();
                        std::process::exit(1);
                    }
                }
            }

            let pairs = pairs.unwrap_or_else(|| {
                eprintln!("Error: pair-plan requires --pairs <n>");
                std::process::exit(1);
            });
            let seed = seed.unwrap_or_else(|| {
                eprintln!("Error: pair-plan requires --seed <u64>");
                std::process::exit(1);
            });
            let plan = pair_plan::build_pair_plan(pairs, seed).unwrap_or_else(|error| {
                eprintln!("Error: {error}");
                std::process::exit(1);
            });
            let json = serde_json::to_string_pretty(&plan).unwrap_or_else(|error| {
                eprintln!("Error: failed to serialize pair plan: {error}");
                std::process::exit(1);
            });

            if let Some(path) = out {
                if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                    if let Err(error) = std::fs::create_dir_all(parent) {
                        eprintln!("Error: failed to create pair-plan directory: {error}");
                        std::process::exit(1);
                    }
                }
                if let Err(error) = std::fs::write(&path, format!("{json}\\n")) {
                    eprintln!("Error: failed to write pair plan: {error}");
                    std::process::exit(1);
                }
                println!("Wrote pair plan to {}", path.display());
            } else {
                println!("{json}");
            }
        }
'''
replace_once(main, anchor, pair_arm + anchor)
