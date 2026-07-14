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

/// Validate that a string is safe to use as a single path component under
/// the repo's `benchmarks/results/` tree.
///
/// `s` arrives from an untrusted manifest on removable media and is joined
/// directly to the repo path to form `dest_dir`. Without validation, a
/// hostile manifest could supply `../..` to escape the ingestion directory,
/// or an absolute path (which `PathBuf::join` silently treats as a new root).
///
/// Returns `Some(s.to_string())` only when `s` is a single safe component:
/// non-empty, no path separators, not `.` or `..`, and containing only
/// ASCII alphanumeric / `_` / `-` / `.` / `:` / `+` (the last two are
/// included because ISO timestamps and host fingerprints use them).
fn safe_segment(s: &str) -> Option<String> {
    if s.is_empty() || s == "." || s == ".." {
        return None;
    }
    // Reject path separators of any kind.
    if s.contains('/') || s.contains('\\') {
        return None;
    }
    // Reject NUL bytes (can't appear in a Rust &str, but defend against
    // future changes that might use &[u8]).
    if s.contains('\0') {
        return None;
    }
    // Reject leading dash: a value like "-flag" could be misinterpreted as
    // a command-line flag by downstream tools (git, jq, etc.). The shell
    // collector enforces the same rule.
    if s.starts_with('-') {
        return None;
    }
    // Allow only the safe character set.
    let all_safe = s.bytes().all(|b| {
        b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b':' || b == b'+'
    });
    if !all_safe {
        return None;
    }
    Some(s.to_string())
}

/// Verify that `child` is strictly under `parent` after canonicalization.
/// Returns `false` if either path cannot be canonicalized (e.g. does not
/// exist yet), which the caller should treat as a containment failure.
///
/// Currently used only by tests, but kept as a public helper so the main
/// flow can adopt it when the ingestion pipeline is refactored to canonicalize
/// the destination after creation (today the main flow uses `starts_with`
/// on pre-canonicalized paths).
#[allow(dead_code)]
fn is_strictly_under(child: &Path, parent: &Path) -> bool {
    let child_c = match child.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let parent_c = match parent.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    child_c == parent_c || child_c.starts_with(&parent_c)
}

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
    println!(
        "  testos-ingest pull /dev/sdX            Mount USB, copy latest results into the repo."
    );
    println!(
        "  testos-ingest format [<date>/<host>]   Generate Markdown summary from latest results."
    );
    println!(
        "  testos-ingest commit                   git add + commit with conventional message."
    );
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
        .args(["-o", "ro", &esp, TMP_MOUNT])
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
            eprintln!(
                "ERROR: no results directory on USB at {}: {}",
                results_root.display(),
                e
            );
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
            eprintln!(
                "ERROR: cannot read manifest {}: {}",
                manifest_path.display(),
                e
            );
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

    let date_part_raw = manifest.started_at.split('T').next().unwrap_or("unknown");
    let host_part_raw = manifest.host.fingerprint.clone();

    // Security (audit finding #2): date_part and host_part come from an
    // untrusted manifest on removable media. They are joined to the repo
    // path and used as the destination for remove_dir_all + file copies.
    // Validate them BEFORE any filesystem mutation to prevent path traversal.
    let date_part = match safe_segment(date_part_raw) {
        Some(s) => s,
        None => {
            eprintln!(
                "ERROR: refusing to use unsafe date segment from manifest: {:?}",
                date_part_raw
            );
            eprintln!("       Expected a date like '2026-07-15'. Possible hostile media.");
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };
    let host_part = match safe_segment(&host_part_raw) {
        Some(s) => s,
        None => {
            eprintln!(
                "ERROR: refusing to use unsafe host fingerprint from manifest: {:?}",
                host_part_raw
            );
            eprintln!("       Expected an ASCII identifier. Possible hostile media.");
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };

    let dest_dir = PathBuf::from(RESULTS_REPO_DIR)
        .join(&date_part)
        .join(&host_part);

    // Defense-in-depth: verify the computed dest_dir is strictly under the
    // repo's benchmarks/results/ directory. safe_segment already validated
    // each component, but a future bug in the join logic (or a new codepath
    // that constructs dest_dir differently) would be caught here instead of
    // causing a destructive remove_dir_all outside the intended tree.
    let results_root = PathBuf::from(RESULTS_REPO_DIR);
    if !results_root.exists() {
        let _ = std::fs::create_dir_all(&results_root);
    }
    // We can't canonicalize dest_dir yet (it may not exist), but we can
    // canonicalize the results_root and check that dest_dir, when joined,
    // starts with the canonical results_root.
    let results_root_canon = match results_root.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "ERROR: cannot canonicalize {}: {}",
                results_root.display(),
                e
            );
            let _ = Command::new("umount").arg(TMP_MOUNT).status();
            std::process::exit(1);
        }
    };
    let dest_dir_canon = results_root_canon.join(&date_part).join(&host_part);
    if !dest_dir_canon.starts_with(&results_root_canon) {
        eprintln!(
            "ERROR: containment check failed: {} is not under {}",
            dest_dir_canon.display(),
            results_root_canon.display()
        );
        let _ = Command::new("umount").arg(TMP_MOUNT).status();
        std::process::exit(1);
    }

    println!("Copying results to {}...", dest_dir.display());
    if dest_dir.exists() {
        eprintln!("WARNING: destination already exists. Removing old contents.");
        // Use the canonicalized path for the removal to ensure we delete
        // exactly what we validated, not a symlink-resolved different path.
        let _ = std::fs::remove_dir_all(&dest_dir_canon);
    }
    if let Err(e) = std::fs::create_dir_all(&dest_dir_canon) {
        eprintln!("ERROR: cannot create {}: {}", dest_dir.display(), e);
        let _ = Command::new("umount").arg(TMP_MOUNT).status();
        std::process::exit(1);
    }

    // Copy each .json file from the latest run dir.
    // Security: validate each filename before joining, and verify the final
    // destination is under dest_dir_canon. A hostile manifest could include
    // a file named "../../../etc/passwd.json" — PathBuf::join would honor
    // the traversal.
    if let Ok(rd) = std::fs::read_dir(&latest) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => {
                        eprintln!(
                            "WARNING: skipping file with non-UTF-8 name: {}",
                            path.display()
                        );
                        continue;
                    }
                };
                // Validate the filename the same way we validate path segments.
                let safe_name = match safe_segment(name) {
                    Some(s) => s,
                    None => {
                        eprintln!(
                            "WARNING: skipping file with unsafe name (possible traversal): {:?}",
                            name
                        );
                        continue;
                    }
                };
                // Require .json extension (already checked above, but
                // safe_segment allows '.' so re-assert).
                if !safe_name.ends_with(".json") {
                    eprintln!("WARNING: skipping non-JSON file: {}", safe_name);
                    continue;
                }
                let dest = dest_dir_canon.join(&safe_name);
                // Defense-in-depth: verify dest is still under dest_dir_canon.
                if !dest.starts_with(&dest_dir_canon) {
                    eprintln!(
                        "WARNING: skipping file whose destination escapes the ingestion dir: {:?}",
                        safe_name
                    );
                    continue;
                }
                // Refuse to follow symlinks (hostile media could symlink
                // results.json -> /etc/shadow; std::fs::copy follows
                // symlinks by default, but the source here is on read-only
                // media so the risk is reading outside the run dir).
                if std::fs::symlink_metadata(&path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    eprintln!(
                        "WARNING: skipping symlink in source tree: {}",
                        path.display()
                    );
                    continue;
                }
                if let Err(e) = std::fs::copy(&path, &dest) {
                    eprintln!("WARNING: failed to copy {}: {}", path.display(), e);
                }
            }
        }
    }

    println!();
    println!(
        "Pulled {} results into {}.",
        manifest.attempted.len(),
        dest_dir.display()
    );
    println!("  Host:    {} ({})", manifest.host.cpu_model, host_part);
    println!("  Date:    {}", date_part);
    println!(
        "  Pass:    {}   Fail: {}   Skip: {}",
        manifest.passed.len(),
        manifest.failed.len(),
        manifest.skipped.len()
    );
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
            eprintln!(
                "ERROR: no results found under {}. Run `testos-ingest pull` first.",
                RESULTS_REPO_DIR
            );
            std::process::exit(1);
        })
    };

    if !results_dir.exists() {
        eprintln!(
            "ERROR: results dir {} does not exist.",
            results_dir.display()
        );
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
            eprintln!(
                "ERROR: cannot read manifest at {}.",
                manifest_path.display()
            );
            std::process::exit(1);
        }
    };

    // Collect per-bench results.
    let mut results: Vec<testos::BenchResult> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&results_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .map(|n| n == "manifest.json")
                .unwrap_or(false)
            {
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
    md.push_str(&format!(
        "# testOS Benchmark Results — {}\n\n",
        manifest.host.fingerprint
    ));
    md.push_str(&format!(
        "- **Date**: {}\n",
        manifest.started_at.split('T').next().unwrap_or("?")
    ));
    md.push_str(&format!("- **Run started**: {}\n", manifest.started_at));
    md.push_str(&format!("- **Run finished**: {}\n", manifest.finished_at));
    md.push_str(&format!("- **Mode**: {}\n", manifest.mode));
    md.push_str(&format!(
        "- **testOS version**: {}\n",
        manifest.testos_version
    ));
    md.push_str(&format!("- **Host CPU**: {}\n", manifest.host.cpu_model));
    md.push_str(&format!("- **Host board**: {}\n", manifest.host.dmi_board));
    md.push_str(&format!("- **Kernel**: {}\n", manifest.host.kernel));
    md.push_str(&format!(
        "- **Battery design (µWh)**: {}\n",
        manifest.host.battery_design_uwh
    ));
    md.push_str(&format!(
        "- **Passed / Failed / Skipped**: {} / {} / {}\n",
        manifest.passed.len(),
        manifest.failed.len(),
        manifest.skipped.len()
    ));
    md.push_str("\n## Results\n\n");
    md.push_str("| Bench | Scenario | Status | Value | Unit | Elapsed | Started |\n");
    md.push_str("|-------|----------|--------|-------|------|---------|---------|\n");
    for r in &results {
        let val = r
            .value
            .map(|v| format!("{}", v))
            .unwrap_or_else(|| "-".to_string());
        let unit = r.unit.clone().unwrap_or_else(|| "-".to_string());
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.1}s | {} |\n",
            r.bench_name,
            r.scenario,
            r.status,
            val,
            unit,
            r.elapsed_seconds,
            r.started_at.split('T').next_back().unwrap_or(&r.started_at)
        ));
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
        eprintln!(
            "ERROR: no results found under {}. Run `testos-ingest pull` first.",
            RESULTS_REPO_DIR
        );
        std::process::exit(1);
    });
    let manifest_path = latest.join("manifest.json");
    let manifest: testos::RunManifest = match std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(m) => m,
        None => {
            eprintln!(
                "ERROR: cannot read manifest at {}.",
                manifest_path.display()
            );
            std::process::exit(1);
        }
    };

    let date = manifest.started_at.split('T').next().unwrap_or("unknown");
    let host_short = &manifest.host.fingerprint[..8.min(manifest.host.fingerprint.len())];
    let message = format!(
        "evidence(bench): testOS run {} host={} pass={} fail={} skip={}",
        date,
        host_short,
        manifest.passed.len(),
        manifest.failed.len(),
        manifest.skipped.len()
    );

    // git add the results dir.
    let rel_path = latest.strip_prefix(".").unwrap_or(&latest);
    let status = Command::new("git")
        .args(["add", &rel_path.to_string_lossy()])
        .status()
        .expect("failed to spawn git");
    if !status.success() {
        eprintln!("git add failed.");
        std::process::exit(1);
    }

    let status = Command::new("git")
        .args(["commit", "-m", &message])
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
        .args(["-ln", "-o", "NAME", device])
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
        .args(["-ln", "-o", "LABEL", part])
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
    let mut dates: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dates.sort();
    let latest_date = dates.last()?;
    let mut hosts: Vec<PathBuf> = std::fs::read_dir(latest_date)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    hosts.sort();
    hosts.last().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_segment_accepts_valid_date() {
        assert_eq!(safe_segment("2026-07-15"), Some("2026-07-15".to_string()));
    }

    #[test]
    fn safe_segment_accepts_valid_host_fingerprint() {
        // Host fingerprints are typically short hex or alphanumeric strings.
        assert_eq!(safe_segment("a1b2c3d4"), Some("a1b2c3d4".to_string()));
        assert_eq!(
            safe_segment("thinkpad-t14-gen4"),
            Some("thinkpad-t14-gen4".to_string())
        );
    }

    #[test]
    fn safe_segment_accepts_timestamp_with_colons() {
        // ISO timestamps like 2026-07-15T10:30:00Z use colons.
        assert_eq!(safe_segment("10:30:00"), Some("10:30:00".to_string()));
    }

    #[test]
    fn safe_segment_rejects_empty() {
        assert_eq!(safe_segment(""), None);
    }

    #[test]
    fn safe_segment_rejects_dot() {
        assert_eq!(safe_segment("."), None);
    }

    #[test]
    fn safe_segment_rejects_dot_dot() {
        assert_eq!(safe_segment(".."), None);
    }

    #[test]
    fn safe_segment_rejects_traversal_with_slash() {
        // These are the exact attack vectors from audit finding #2.
        assert_eq!(safe_segment("../"), None);
        assert_eq!(safe_segment("../.."), None);
        assert_eq!(safe_segment("../etc/passwd"), None);
        assert_eq!(safe_segment("a/b"), None);
        assert_eq!(safe_segment("/etc/passwd"), None);
        assert_eq!(safe_segment("/"), None);
    }

    #[test]
    fn safe_segment_rejects_backslash_traversal() {
        assert_eq!(safe_segment("..\\..\\windows"), None);
        assert_eq!(safe_segment("a\\b"), None);
    }

    #[test]
    fn safe_segment_rejects_nul_byte() {
        assert_eq!(safe_segment("a\0b"), None);
        assert_eq!(safe_segment("\0"), None);
    }

    #[test]
    fn safe_segment_rejects_spaces_and_shell_metacharacters() {
        // A hostile manifest could try to inject shell arguments.
        assert_eq!(safe_segment("my app"), None);
        assert_eq!(safe_segment("app;rm -rf /"), None);
        assert_eq!(safe_segment("app|cat"), None);
        assert_eq!(safe_segment("app$HOME"), None);
        assert_eq!(safe_segment("app`whoami`"), None);
        assert_eq!(safe_segment("app\nrm"), None);
    }

    #[test]
    fn safe_segment_rejects_leading_dash() {
        // Dash-leading values could be misinterpreted as flags by
        // downstream tools. The shell collector rejects these too.
        assert_eq!(safe_segment("-flag"), None);
        assert_eq!(safe_segment("--global"), None);
    }

    #[test]
    fn safe_segment_accepts_max_reasonable_length() {
        // 64 chars is a reasonable upper bound for a host fingerprint.
        let s = "a".repeat(64);
        assert_eq!(safe_segment(&s), Some(s));
    }

    /// Regression test for the audit's exact attack scenario: a hostile
    /// manifest provides a date_part or host_fingerprint that, when joined
    /// to the results root, would escape the ingestion directory.
    #[test]
    fn safe_segment_blocks_audit_attack_vectors() {
        // From the audit: "The shell collector permits traversal components
        // before rm -rf; the Rust implementation's PathBuf::join also
        // accepts absolute components and then invokes remove_dir_all."
        assert_eq!(safe_segment("../.."), None);
        assert_eq!(safe_segment("../../etc"), None);
        assert_eq!(safe_segment("/etc"), None);
        assert_eq!(safe_segment("/run/optid"), None);
    }

    #[test]
    fn is_strictly_under_accepts_descendant() {
        let tmp = std::env::temp_dir().join(format!(
            "testos_ingest_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let child = tmp.join("a").join("b");
        std::fs::create_dir_all(&child).unwrap();
        assert!(is_strictly_under(&child, &tmp));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn is_strictly_under_rejects_sibling() {
        let tmp_parent = std::env::temp_dir().join(format!(
            "testos_ingest_parent_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let tmp_sibling = std::env::temp_dir().join(format!(
            "testos_ingest_sibling_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_parent).unwrap();
        std::fs::create_dir_all(&tmp_sibling).unwrap();
        // sibling is NOT under parent.
        assert!(!is_strictly_under(&tmp_sibling, &tmp_parent));
        std::fs::remove_dir_all(&tmp_parent).ok();
        std::fs::remove_dir_all(&tmp_sibling).ok();
    }

    #[test]
    fn is_strictly_under_rejects_traversal_string() {
        // Construct a path string that contains ".." but points to a real
        // dir outside the parent. is_strictly_under must reject it.
        let tmp = std::env::temp_dir().join(format!(
            "testos_ingest_trav_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let parent = tmp.join("parent");
        let sibling = tmp.join("sibling");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        // "../sibling" from inside parent/ resolves to tmp/sibling, which is
        // NOT under parent.
        let escaping = parent.join("..").join("sibling");
        assert!(!is_strictly_under(&escaping, &parent));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
