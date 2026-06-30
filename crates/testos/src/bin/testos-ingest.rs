//! testos-ingest — runs on the HOST after the test machine has rebooted back.
//!
//! Responsibilities:
//! 1. `testos-ingest pull <device>` — mount the USB, find the latest results dir,
//!    copy it into the repo at benchmarks/results/<UTC-date>/<host-fingerprint>/.
//! 2. `testos-ingest format` — generate a Markdown summary from the pulled results.
//! 3. `testos-ingest commit` — git add + git commit with a conventional commit message.
//!
//! This is the last step of the workflow. After this, results are in the repo
//! and can be pushed.

use std::path::{Path, PathBuf};
use std::process::Command;

const RESULTS_REPO_DIR: &str = "benchmarks/results";
const TMP_MOUNT: &str = "/mnt/testos-ingest";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "pull" => cmd_pull(&args[2..]),
        "format" => cmd_format(&args[2..]),
        "commit" => cmd_commit(&args[2..]),
        "--help" | "-h" | "help" => print_usage(),
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!("testOS ingest — pull results from USB, format, commit to repo.");
    println!();
    println!("Usage:");
    println!("  testos-ingest pull /dev/sdX            Mount USB, copy latest results into the repo.");
    println!("  testos-ingest format [<date>/<host>]   Generate Markdown summary from latest results.");
    println!("  testos-ingest commit                   git add + commit with conventional message.");
    println!();
    println!("Typical flow (after testOS has rebooted the test machine back to host):");
    println!("  1. testos-ingest pull /dev/sdX");
    println!("  2. testos-ingest format");
    println!("  3. testos-ingest commit");
}

fn cmd_pull(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: testos-ingest pull /dev/sdX");
        std::process::exit(1);
    }
    let device = &args[0];

    // Find the testOS partition (label RUSHESP) on this device.
    let parts = find_partitions(device);
    let mut esp_path: Option<String> = None;
    for p in &parts {
        if let Some(label) = read_partition_label(p) {
            if label == "RUSHESP" {
                esp_path = Some(p.clone());
                break;
            }
        }
    }
    let esp = match esp_path {
        Some(p) => p,
        None => {
            eprintln!("ERROR: no partition labeled RUSHESP found on {}.", device);
            eprintln!("       Is this the right USB? Was it written with `testos-launcher write`?");
            std::process::exit(1);
        }
    };

    // Mount the ESP read-only first (safer; we only need to read results).
    let _ = std::fs::create_dir_all(TMP_MOUNT);
    let mount_ok = Command::new("mount")
        .args(&["-o", "ro", &esp, TMP_MOUNT])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !mount_ok {
        // Maybe it was already mounted from a previous run. Try to use it as-is.
        if !Path::new(TMP_MOUNT).join("testos-results").exists() {
            eprintln!("ERROR: failed to mount {} at {}", esp, TMP_MOUNT);
            std::process::exit(1);
        }
    }

    // Find the latest results dir.
    let results_root = Path::new(TMP_MOUNT).join("testos-results");
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&results_root) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(e) => {
            eprintln!("ERROR: no results directory on USB at {}: {}", results_root.display(), e);
            eprintln!("       Did testOS actually run any benchmarks?");
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };
    entries.sort();
    let latest = match entries.last() {
        Some(p) => p.clone(),
        None => {
            eprintln!("ERROR: results directory is empty.");
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };

    println!("Latest results: {}", latest.display());

    // Read the manifest to get host fingerprint and date.
    let manifest_path = latest.join("manifest.json");
    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERROR: cannot read manifest {}: {}", manifest_path.display(), e);
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };
    let manifest: testos::RunManifest = match serde_json::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ERROR: cannot parse manifest: {}", e);
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };

    let date_part = manifest.started_at.split('T').next().unwrap_or("unknown");
    let host_part = manifest.host.fingerprint.clone();
    let dest_dir = PathBuf::from(RESULTS_REPO_DIR).join(date_part).join(&host_part);

    println!("Copying results to {}...", dest_dir.display());
    if dest_dir.exists() {
        eprintln!("WARNING: destination already exists. Removing old contents.");
        let _ = std::fs::remove_dir_all(&dest_dir);
    }
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        eprintln!("ERROR: cannot create {}: {}", dest_dir.display(), e);
        let _ = Command::new("umount").arg(TMP_MOUNT).status();
        std::process::exit(1);
    }

    // Copy each .json file from the latest run dir.
    if let Ok(rd) = std::fs::read_dir(&latest) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let name = path.file_name().unwrap();
                let dest = dest_dir.join(name);
                if let Err(e) = std::fs::copy(&path, &dest) {
                    eprintln!("WARNING: failed to copy {}: {}", path.display(), e);
                }
            }
        }
    }

    println!();
    println!("Pulled {} results into {}.", manifest.attempted.len(), dest_dir.display());
    println!("  Host:    {} ({})", manifest.host.cpu_model, host_part);
    println!("  Date:    {}", date_part);
    println!("  Pass:    {}   Fail: {}   Skip: {}", manifest.passed.len(), manifest.failed.len(), manifest.skipped.len());
    println!();
    println!("Next: format and commit:");
    println!("  testos-ingest format {}/{}", date_part, host_part);
    println!("  testos-ingest commit");

    let _ = Command::new("umount").arg(TMP_MOUNT).status();
}

fn cmd_format(args: &[String]) {
    // Resolve the results dir.
    let results_dir = if !args.is_empty() {
        PathBuf::from(RESULTS_REPO_DIR).join(&args[0])
    } else {
        // Find the latest results dir under benchmarks/results/.
        find_latest_results_dir().unwrap_or_else(|| {
            eprintln!("ERROR: no results found under {}. Run `testos-ingest pull` first.", RESULTS_REPO_DIR);
            std::process::exit(1);
        })
    };

    if !results_dir.exists() {
        eprintln!("ERROR: results dir {} does not exist.", results_dir.display());
        std::process::exit(1);
    }

    // Read the manifest.
    let manifest_path = results_dir.join("manifest.json");
    let manifest: testos::RunManifest = match std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(m) => m,
        None => {
            eprintln!("ERROR: cannot read manifest at {}.", manifest_path.display());
            std::process::exit(1);
        }
    };

    // Collect per-bench results.
    let mut results: Vec<testos::BenchResult> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&results_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.file_name().map(|n| n == "manifest.json").unwrap_or(false) {
                continue;
            }
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(r) = serde_json::from_str::<testos::BenchResult>(&text) {
                        results.push(r);
                    }
                }
            }
        }
    }

    // Generate Markdown.
    let mut md = String::new();
    md.push_str(&format!("# testOS Benchmark Results — {}\n\n", manifest.host.fingerprint));
    md.push_str(&format!("- **Date**: {}\n", manifest.started_at.split('T').next().unwrap_or("?")));
    md.push_str(&format!("- **Run started**: {}\n", manifest.started_at));
    md.push_str(&format!("- **Run finished**: {}\n", manifest.finished_at));
    md.push_str(&format!("- **Mode**: {}\n", manifest.mode));
    md.push_str(&format!("- **testOS version**: {}\n", manifest.testos_version));
    md.push_str(&format!("- **Host CPU**: {}\n", manifest.host.cpu_model));
    md.push_str(&format!("- **Host board**: {}\n", manifest.host.dmi_board));
    md.push_str(&format!("- **Kernel**: {}\n", manifest.host.kernel));
    md.push_str(&format!("- **Battery design (µWh)**: {}\n", manifest.host.battery_design_uwh));
    md.push_str(&format!("- **Passed / Failed / Skipped**: {} / {} / {}\n",
        manifest.passed.len(), manifest.failed.len(), manifest.skipped.len()));
    md.push_str("\n## Results\n\n");
    md.push_str("| Bench | Scenario | Status | Value | Unit | Elapsed | Started |\n");
    md.push_str("|-------|----------|--------|-------|------|---------|---------|\n");
    for r in &results {
        let val = r.value.map(|v| format!("{}", v)).unwrap_or_else(|| "-".to_string());
        let unit = r.unit.clone().unwrap_or_else(|| "-".to_string());
        md.push_str(&format!("| {} | {} | {} | {} | {} | {:.1}s | {} |\n",
            r.bench_name, r.scenario, r.status, val, unit, r.elapsed_seconds,
            r.started_at.split('T').last().unwrap_or(&r.started_at)));
    }

    if !manifest.failed.is_empty() {
        md.push_str("\n## Failures\n\n");
        for r in &results {
            if r.status == "fail" {
                md.push_str(&format!("### {}\n\n", r.bench_name));
                md.push_str(&format!("- Exit code: {:?}\n", r.exit_code));
                md.push_str(&format!("- Started: {}\n", r.started_at));
                if let Some(s) = &r.stderr {
                    let trimmed = if s.len() > 2000 { &s[..2000] } else { s };
                    md.push_str("\n```\n");
                    md.push_str(trimmed);
                    md.push_str("\n```\n");
                }
                md.push('\n');
            }
        }
    }

    let md_path = results_dir.join("SUMMARY.md");
    if let Err(e) = std::fs::write(&md_path, &md) {
        eprintln!("ERROR: failed to write {}: {}", md_path.display(), e);
        std::process::exit(1);
    }
    println!("Wrote Markdown summary: {}", md_path.display());
    println!();
    println!("Preview:");
    println!("  cat {}", md_path.display());
    println!();
    println!("Next: commit with `testos-ingest commit`.");
}

fn cmd_commit(_args: &[String]) {
    // Find the latest results dir.
    let latest = find_latest_results_dir().unwrap_or_else(|| {
        eprintln!("ERROR: no results found under {}. Run `testos-ingest pull` first.", RESULTS_REPO_DIR);
        std::process::exit(1);
    });
    let manifest_path = latest.join("manifest.json");
    let manifest: testos::RunManifest = match std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(m) => m,
        None => {
            eprintln!("ERROR: cannot read manifest at {}.", manifest_path.display());
            std::process::exit(1);
        }
    };

    let date = manifest.started_at.split('T').next().unwrap_or("unknown");
    let host_short = &manifest.host.fingerprint[..8.min(manifest.host.fingerprint.len())];
    let message = format!(
        "evidence(bench): testOS run {} host={} pass={} fail={} skip={}",
        date, host_short, manifest.passed.len(), manifest.failed.len(), manifest.skipped.len()
    );

    // git add the results dir.
    let rel_path = latest.strip_prefix(".").unwrap_or(&latest);
    let status = Command::new("git")
        .args(&["add", &rel_path.to_string_lossy()])
        .status()
        .expect("failed to spawn git");
    if !status.success() {
        eprintln!("git add failed.");
        std::process::exit(1);
    }

    let status = Command::new("git")
        .args(&["commit", "-m", &message])
        .status()
        .expect("failed to spawn git");
    if !status.success() {
        eprintln!("git commit failed (maybe nothing to commit, or pre-commit hook rejected).");
        std::process::exit(1);
    }

    println!();
    println!("Committed: {}", message);
    println!();
    println!("Push with: git push");
}

fn find_partitions(device: &str) -> Vec<String> {
    let out = Command::new("lsblk")
        .args(&["-ln", "-o", "NAME", device])
        .output();
    let mut parts = Vec::new();
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let name = line.trim();
            if name.is_empty() {
                continue;
            }
            let dev_name = device.strip_prefix("/dev/").unwrap_or(device);
            if name == dev_name {
                continue;
            }
            if name.starts_with(dev_name) {
                parts.push(format!("/dev/{}", name));
            }
        }
    }
    parts
}

fn read_partition_label(part: &str) -> Option<String> {
    let out = Command::new("lsblk")
        .args(&["-ln", "-o", "LABEL", part])
        .output()
        .ok()?;
    let label = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

fn find_latest_results_dir() -> Option<PathBuf> {
    let root = Path::new(RESULTS_REPO_DIR);
    let mut dates: Vec<PathBuf> = std::fs::read_dir(root).ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dates.sort();
    let latest_date = dates.last()?;
    let mut hosts: Vec<PathBuf> = std::fs::read_dir(latest_date).ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    hosts.sort();
    hosts.last().cloned()
}
