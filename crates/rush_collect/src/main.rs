mod collect;
mod platform;
mod types;

use std::env;
use std::fs;

fn print_usage() {
    eprintln!("rush-collect — passive hardware profile + metric snapshot");
    eprintln!();
    eprintln!("Collects a 30-second observation window and outputs a single JSON");
    eprintln!("object. No root required. Makes no changes to the system.");
    eprintln!();
    eprintln!("Usage: rush-collect [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --window-sec <N>   Observation window length (default: 30)");
    eprintln!("  --out <path>       Write JSON to file; default is stdout");
    eprintln!("  --help             Show this message");
    eprintln!();
    eprintln!("What it reads (Linux):");
    eprintln!("  /proc/cpuinfo, /proc/meminfo, /proc/loadavg");
    eprintln!("  /proc/pressure/{{cpu,io}}   (PSI — kernel >= 4.20)");
    eprintln!("  /sys/class/powercap/intel-rapl:0/energy_uj");
    eprintln!("  /sys/class/power_supply/BAT*/energy_now");
    eprintln!("  /sys/class/thermal/thermal_zone*/temp");
    eprintln!("  /sys/devices/system/cpu/cpu*/cpufreq/scaling_*");
    eprintln!("  /sys/class/dmi/id/chassis_type");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut window_sec: u64 = 30;
    let mut out: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--window-sec" => {
                window_sec = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| {
                        eprintln!("Error: --window-sec requires a positive integer");
                        std::process::exit(1);
                    });
                i += 2;
            }
            "--out" => {
                out = Some(
                    args.get(i + 1)
                        .cloned()
                        .unwrap_or_else(|| {
                            eprintln!("Error: --out requires a file path");
                            std::process::exit(1);
                        }),
                );
                i += 2;
            }
            "--help" | "-h" => {
                print_usage();
                return;
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    let record = collect::run(window_sec);

    let json = serde_json::to_string_pretty(&record).expect("serialization failed");

    match out {
        Some(ref path) => {
            fs::write(path, &json).unwrap_or_else(|e| {
                eprintln!("Error writing to {path}: {e}");
                std::process::exit(1);
            });
            eprintln!("rush-collect: written to {path}");
        }
        None => println!("{}", json),
    }
}
