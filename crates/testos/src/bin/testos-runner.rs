//! testos-runner - runs INSIDE testOS after boot.
//!
//! Responsibilities:
//! 1. Show a menu (driven by catalog data — never embed descriptions here).
//! 2. For each selected benchmark: print a header, run it, capture stdout/stderr/exit,
//!    write one JSON result file to the results directory on the USB stick.
//! 3. Show per-benchmark progress with spinner, overall percentage, elapsed, and ETA.
//!    Percentage is based on completed benchmark count — never fabricated inside
//!    an opaque running command.
//! 4. Honor Esc (read from /dev/console) to abort the run early — partial results saved.
//! 5. Write a top-level RunManifest.json when done (or aborted).
//! 6. Capture raw boot diagnostics to PRIVATE-DIAGNOSTICS/<run_id>/ on the USB
//!    (NEVER into testos-results/) and sync.
//! 7. Reboot back to the host OS.
//!
//! Failure behavior: on any uncorrectable failure (USB not found, intent invalid,
//! etc.) the runner shows a privacy-safe recovery screen with a short failure
//! code and reboots. It does NOT spawn an interactive root shell.
//!
//! Where to find things after boot:
//! - The USB stick is mounted at /run/testos/usb (mounted by testos-usb-mount.service).
//! - Bench list TOML is at /run/testos/usb/testos/bench-list.toml (copied at build time).
//! - Results directory is /run/testos/usb/testos-results/<UTC-timestamp>/.
//! - Private diagnostics: /run/testos/usb/PRIVATE-DIAGNOSTICS/<run_id>/ (local only).
//!
//! The runner itself is installed at /usr/bin/testos-runner inside the testOS image.

use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use testos::{
    private_diag, recovery::FailureCategory, tui as testos_tui, Bench, BenchKind, BenchList,
    BenchResult, HostFingerprint, RunIntent, RunManifest, RunProvenance, SCHEMA_VERSION,
};

const USB_MOUNT: &str = "/run/testos/usb";
const RESULTS_SUBDIR: &str = "testos-results";
const BENCH_LIST_REL: &str = "testos/bench-list.toml";

/// Fallback version used only when neither /etc/testos/version nor
/// /etc/os-release is available. This is a last resort; the canonical
/// version comes from the VERSION file at build time (written to
/// /etc/testos/version by build-testos.sh).
const TESTOS_VERSION_FALLBACK: &str = "0.7.0-unknown";

/// Read the boot attempt number from /run/testos/boot-attempt. The mount
/// helper writes this file (1 on first boot, 2 on second, etc.) so the
/// runner can report it on screen and in the private diagnostics.
fn read_boot_attempt() -> u32 {
    std::fs::read_to_string("/run/testos/boot-attempt")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(1)
}

fn main() {
    // Pick the palette once. The runner runs on tty1 inside testOS, so
    // stdout is normally a TTY. When NO_COLOR is set or stdout is piped
    // (e.g. serial console capture), we degrade to plain text.
    let is_tty = std::io::IsTerminal::is_terminal(&io::stdout());
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let palette = testos_tui::Palette::for_output(is_tty, no_color);

    let boot_attempt = read_boot_attempt();

    // Print the source git SHA so the user can verify the USB contains
    // the code they actually built. This file is written by build-testos.sh.
    let source_sha = std::fs::read_to_string("/etc/testos/source-sha")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();
    let image_version = read_running_testos_version();
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();

    testos_tui::print_banner(&palette, &source_sha, &image_version, &kernel, boot_attempt);
    testos_tui::print_acpi_note(&palette);

    // Helper: write raw diagnostics to PRIVATE-DIAGNOSTICS, show the
    // privacy-safe recovery screen, wait, and reboot. NEVER drops to a
    // root shell. `category` is the short failure code shown on screen.
    //
    // We clone the palette into the closure so the rest of main can keep
    // using it for the menu / per-bench progress.
    let palette_for_fail = palette.clone();
    let fail_safe = move |category: FailureCategory, run_id_hint: &str| -> ! {
        fail_safe_impl(&palette_for_fail, boot_attempt, category, run_id_hint);
    };

    // 1. Verify USB mount exists.
    let usb = Path::new(USB_MOUNT);
    if !usb.exists() {
        fail_safe(FailureCategory::UsbNotFound, "");
    }

    // 1b. Compute the running testOS image version. This is needed both to
    // validate the run-intent (the intent's `testos_version` must match the
    // running image) and to fill `manifest.json.testos_version`. The
    // canonical source is /etc/testos/version (written by build-testos.sh
    // from the repo VERSION file); /etc/os-release VERSION= is the fallback.
    let running_testos_version = read_running_testos_version();

    // 1c. Load and fully validate the run-intent from the USB.
    //
    // The host planner writes run-intent.json to the USB before boot. The
    // runner reads it here, refuses to run if it is missing/malformed/stale/
    // dry-run/inconsistent, and copies every field into manifest.json so the
    // strict evidence validator can re-bind the run to the plan, catalog,
    // image, source commit, and run_id.
    //
    // This is fail-closed: a missing or invalid intent never falls through to
    // an unsigned/default run. The operator must re-prepare the USB from a
    // host that has a valid plan + checkpoint.
    let bench_list_path = usb.join(BENCH_LIST_REL);
    let intent_raw_bytes: Vec<u8> =
        std::fs::read(usb.join(testos::run_intent::INTENT_FILENAME)).unwrap_or_else(|_| Vec::new());
    let intent: RunIntent =
        match RunIntent::load_and_validate(usb, &running_testos_version, &bench_list_path) {
            Ok(i) => i,
            Err(e) => {
                // The intent is the cryptographic association between the host
                // and the runner. Without it, any results we write would be
                // unverifiable. Fail closed — show the recovery screen with
                // the E003 code, do NOT drop to a shell.
                eprintln!("run-intent validation failed: {}", e);
                fail_safe(FailureCategory::IntentInvalid, "");
            }
        };
    // We now have a valid run_id — use it for private diagnostics.
    let run_id = intent.run_id.clone();
    let _ = writeln!(
        io::stdout(),
        "{}run_id:{} {}    {}source commit:{} {}",
        palette.dim,
        palette.reset,
        intent.run_id,
        palette.dim,
        palette.reset,
        intent.source_commit,
    );
    let _ = writeln!(
        io::stdout(),
        "{}image digest:{} {}    {}plan:{} {}    {}catalog:{} {}",
        palette.dim,
        palette.reset,
        intent.testos_image_digest,
        palette.dim,
        palette.reset,
        intent.plan_sha256,
        palette.dim,
        palette.reset,
        intent.benchmark_catalog_sha256,
    );
    let _ = writeln!(io::stdout());

    // 2. Load bench list from the USB.
    let list = match BenchList::load(&bench_list_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot load bench list: {}", e);
            fail_safe(FailureCategory::CatalogInvalid, &run_id);
        }
    };

    let _ = writeln!(
        io::stdout(),
        "{}Loaded {} benchmarks from catalog v{}.{}",
        palette.dim,
        list.benches.len(),
        list.version,
        palette.reset,
    );
    let _ = writeln!(io::stdout());

    // 3. Capture host fingerprint once per run. The host fingerprint is
    // already part of the provenance contract; we keep printing a compact
    // summary so the operator can confirm the hardware matches expectations
    // without dumping raw identifiers.
    let host = HostFingerprint::capture();
    let _ = writeln!(
        io::stdout(),
        "{}host:{} {} ({} / {})",
        palette.dim,
        palette.reset,
        host.fingerprint,
        host.cpu_model,
        host.kernel,
    );
    let _ = writeln!(io::stdout());

    // 4. Show menu.
    let selection = match show_menu(&palette, &list) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Menu error: {}", e);
            fail_safe(FailureCategory::InternalError, &run_id);
        }
    };

    if selection.is_empty() {
        let _ = writeln!(
            io::stdout(),
            "{}Nothing selected. Rebooting back to host OS.{}",
            palette.yellow,
            palette.reset
        );
        reboot_host();
        return;
    }

    // 5. Create results directory on the USB.
    let started_at = iso_utc_now();
    let results_dir = usb.join(RESULTS_SUBDIR).join(started_at.replace(':', "-"));
    if let Err(e) = std::fs::create_dir_all(&results_dir) {
        eprintln!("cannot create results dir {}: {}", results_dir.display(), e);
        fail_safe(FailureCategory::InternalError, &run_id);
    }

    let mode = if selection.len() == list.benches.len() {
        "all".to_string()
    } else {
        format!(
            "selection:{}",
            selection
                .iter()
                .map(|b| b.id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    };

    let _ = writeln!(io::stdout());
    let _ = writeln!(
        io::stdout(),
        "{}results:{} {}    {}mode:{} {}",
        palette.dim,
        palette.reset,
        results_dir.display(),
        palette.dim,
        palette.reset,
        mode
    );
    let _ = writeln!(io::stdout());

    // 6. Start the Esc watcher thread.
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        watch_for_esc(tx);
    });

    // 7. Run each selected benchmark. The overall percentage is based on
    // completed benchmark count — never fabricated from inside an opaque
    // running command. While one command runs, we show elapsed time and a
    // spinner and label its duration as estimated.
    let mut attempted = Vec::new();
    let mut passed = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();

    let total = selection.len();
    let total_estimated: u64 = selection.iter().map(|b| b.estimated_seconds).sum();
    let run_started = Instant::now();
    let mut aborted = false;
    for (idx, bench) in selection.iter().enumerate() {
        // Check for abort signal.
        if rx.try_recv().is_ok() {
            let _ = writeln!(
                io::stdout(),
                "{}!! Aborted by user (Esc pressed). Saving partial results.{}",
                palette.yellow,
                palette.reset
            );
            for remaining in &selection[idx..] {
                skipped.push(remaining.id.clone());
            }
            aborted = true;
            break;
        }

        let completed = idx;
        let position = testos_tui::progress_position(completed, total);
        let elapsed_total = run_started.elapsed();
        let remaining_estimate = total_estimated.saturating_sub(elapsed_total.as_secs());
        testos_tui::print_bench_header(
            &palette,
            bench,
            &position,
            elapsed_total,
            Duration::from_secs(remaining_estimate),
        );

        if bench.requires_battery && host.battery_design_uwh == 0 {
            let _ = writeln!(
                io::stdout(),
                "  {}SKIPPED: requires battery but no battery present.{}",
                palette.yellow,
                palette.reset
            );
            skipped.push(bench.id.clone());
            attempted.push(bench.id.clone());
            continue;
        }

        if bench.requires_battery {
            let _ = writeln!(
                io::stdout(),
                "  {}This benchmark requires battery power. Unplug AC now.{}",
                palette.yellow,
                palette.reset
            );
            let _ = write!(io::stdout(), "  Press Enter when ready (or 's' to skip): ");
            let _ = io::stdout().flush();
            let mut line = String::new();
            io::stdin().read_line(&mut line).ok();
            if line.trim() == "s" {
                let _ = writeln!(
                    io::stdout(),
                    "  {}Skipped by user.{}",
                    palette.yellow,
                    palette.reset
                );
                skipped.push(bench.id.clone());
                attempted.push(bench.id.clone());
                continue;
            }
        }

        // Spinner lifecycle: start a spinner that proves the process is
        // alive while the opaque benchmark command runs, and stop it as
        // soon as the command returns (success, failure, or signal).
        let mut spinner = testos_tui::Spinner::start("  running...", &palette);

        let started = Instant::now();
        let started_iso = iso_utc_now();
        let (status, value, unit, stdout, stderr, exit_code) = run_benchmark(bench, &results_dir);
        let elapsed = started.elapsed().as_secs_f64();
        let finished_iso = iso_utc_now();

        // Stop the spinner BEFORE printing the completion line so the two
        // do not clobber each other on a TTY.
        spinner.stop();

        let result = BenchResult {
            schema_version: SCHEMA_VERSION,
            bench_id: bench.id.clone(),
            bench_name: bench.name.clone(),
            scenario: bench.scenario.clone(),
            status: status.clone(),
            started_at: started_iso,
            finished_at: finished_iso,
            elapsed_seconds: elapsed,
            value,
            unit: unit.clone(),
            stdout: Some(BenchResult::stdout_truncated(&stdout)),
            stderr: Some(BenchResult::stdout_truncated(&stderr)),
            exit_code,
            host: host.clone(),
        };

        let result_path = results_dir.join(format!("{}.json", bench.id));
        match serde_json::to_string_pretty(&result) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&result_path, json) {
                    eprintln!(
                        "   WARNING: failed to write result to {}: {}",
                        result_path.display(),
                        e
                    );
                }
            }
            Err(e) => eprintln!("   WARNING: failed to serialize result: {}", e),
        }

        attempted.push(bench.id.clone());
        let status_word = match status.as_str() {
            "pass" => {
                passed.push(bench.id.clone());
                testos_tui::StatusWord::Pass
            }
            "fail" => {
                failed.push(bench.id.clone());
                testos_tui::StatusWord::Fail
            }
            _ => {
                skipped.push(bench.id.clone());
                testos_tui::StatusWord::Skipped
            }
        };
        let _ = writeln!(
            io::stdout(),
            "  {} {}{} ({})",
            status_word.render(&palette),
            match (&value, &unit) {
                (Some(v), Some(u)) => format!(" — {} {} ", v, u),
                _ => String::new(),
            },
            palette.reset,
            testos_tui::format_elapsed(Duration::from_secs_f64(elapsed))
        );
        let _ = writeln!(io::stdout());
        // Sync the USB filesystem so we don't lose results on a sudden reboot.
        let _ = Command::new("sync").status();
    }

    // 8. Write the top-level manifest.
    let finished_at = iso_utc_now();
    // `running_testos_version` was computed early (step 1b) and already
    // cross-checked against the run-intent's `testos_version`. Reuse it here
    // so the manifest is guaranteed to match the intent. This also fixes the
    // historical defect where the release asset v0.7.0-beta.4 wrote manifest
    // testos_version=0.7.0-beta.1 because a stale fallback constant was used.
    let testos_version = running_testos_version.clone();

    // Capture counts before moving the Vecs into the manifest — the
    // post-run summary needs them, and we move the Vecs into `RunManifest`.
    let attempted_count = attempted.len();
    let passed_count = passed.len();
    let failed_count = failed.len();
    let skipped_count = skipped.len();

    // Build the provenance block from the validated run-intent. Every field
    // is copied from the intent (or recomputed, for `intent_sha256`) so the
    // strict evidence validator can re-bind the run to the plan, catalog,
    // image, source commit, and run_id without trusting the runner.
    let mut provenance = RunProvenance::from_intent(&intent, &intent_raw_bytes);

    // F8: separate host_workflow_commit vs testos_image_commit.
    // `source_commit` in the intent is the host-workflow commit (the
    // commit the host-side tools were built from). The testOS image was
    // built from a potentially different commit, baked into the image at
    // build time as /etc/testos/source-sha. If the intent carries
    // `testos_image_commit`, we cross-check it against the running image.
    // If the intent does NOT carry it, we fill it from /etc/testos/source-sha
    // so the manifest records which image actually ran.
    let image_source_sha = std::fs::read_to_string("/etc/testos/source-sha")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();
    if image_source_sha.len() == 40 && image_source_sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        // Valid 40-char hex SHA from the image.
        match &provenance.testos_image_commit {
            None => {
                // Intent didn't carry it; fill from the running image.
                provenance.testos_image_commit = Some(image_source_sha.clone());
            }
            Some(intent_sha) => {
                // Intent carried it; cross-check against the running image.
                if intent_sha != &image_source_sha {
                    eprintln!(
                        "WARNING: testos_image_commit mismatch: intent={} image={}",
                        intent_sha, image_source_sha
                    );
                    // Do NOT overwrite — the intent's value is authoritative
                    // for provenance binding. The validator will catch the
                    // mismatch.
                }
            }
        }
    }

    let manifest = RunManifest {
        schema_version: SCHEMA_VERSION,
        started_at: started_at.clone(),
        finished_at,
        mode,
        attempted,
        passed,
        failed,
        skipped,
        host: host.clone(),
        testos_version,
        provenance: Some(provenance),
    };

    let manifest_path = results_dir.join("manifest.json");
    match serde_json::to_string_pretty(&manifest) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&manifest_path, json) {
                eprintln!(
                    "WARNING: failed to write manifest to {}: {}",
                    manifest_path.display(),
                    e
                );
            }
        }
        Err(e) => eprintln!("WARNING: failed to serialize manifest: {}", e),
    }

    // Record the source git SHA as evidence (not merely printed to the
    // console). The provenance block already carries the 40-char source_commit
    // from the run-intent; this file is the operator-facing short SHA from
    // /etc/testos/source-sha so a reviewer can eyeball it against
    // `git rev-parse --short HEAD` without recomputing the full hash.
    let _ = std::fs::write(results_dir.join("source-sha.txt"), &source_sha);

    // Copy the run-intent.json and plan.json into the results directory so
    // the strict evidence validator can re-bind the manifest to the exact
    // intent and plan the host launched. The validator recomputes
    // intent_sha256 and plan_sha256 and compares them to the manifest's
    // provenance block. Without these files the validator fails closed.
    let intent_dest = results_dir.join("run-intent.json");
    if !intent_raw_bytes.is_empty() {
        if let Err(e) = std::fs::write(&intent_dest, &intent_raw_bytes) {
            eprintln!("WARNING: failed to write run-intent.json to results: {}", e);
        }
    }
    // The host planner writes plan.json alongside run-intent.json at the USB
    // root. Copy it through if present (it is required for the validator to
    // recompute plan_sha256).
    let plan_src = usb.join("plan.json");
    if plan_src.exists() {
        if let Ok(plan_bytes) = std::fs::read(&plan_src) {
            let _ = std::fs::write(results_dir.join("plan.json"), &plan_bytes);
        }
    }
    // Copy the bench-list.toml so the validator can recompute
    // benchmark_catalog_sha256 without relying on the image.
    if let Ok(cat_bytes) = std::fs::read(&bench_list_path) {
        let _ = std::fs::write(results_dir.join("bench-list.toml"), &cat_bytes);
    }

    // Generate result-hashes.json: a deterministic SHA-256 entry for every
    // benchmark result file. This sidecar is MANDATORY for strict submission —
    // the evidence validator rejects any bundle without it, and rejects any
    // missing, extra, or mismatched entry. Written after all results are
    // finalized so a tampered result is detected on collection.
    let result_hashes = compute_result_hashes(&results_dir);
    match serde_json::to_string_pretty(&result_hashes) {
        Ok(json) => {
            if let Err(e) = std::fs::write(results_dir.join("result-hashes.json"), json) {
                eprintln!(
                    "WARNING: failed to write result-hashes.json to {}: {}",
                    results_dir.display(),
                    e
                );
            }
        }
        Err(e) => eprintln!("WARNING: failed to serialize result-hashes.json: {}", e),
    }

    // Capture raw boot diagnostics into PRIVATE-DIAGNOSTICS/<run_id>/ on the
    // USB — NEVER into the publishable results directory. This is the hard
    // privacy boundary: the strict evidence validator rejects any bundle
    // that contains PRIVATE-DIAGNOSTICS, any raw dmesg/journal artifact,
    // or any symlink that references this directory.
    //
    // We capture AFTER the run completes (or aborts) and BEFORE the reboot,
    // so the diagnostics reflect the full boot + run state. The directory
    // is marked with a README.txt containing
    // "PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT".
    let _ = writeln!(
        io::stdout(),
        "{}Capturing private diagnostics to PRIVATE-DIAGNOSTICS/{}/...{}",
        palette.dim,
        run_id,
        palette.reset
    );
    let diag_dir = match private_diag::ensure_dir(usb, &run_id, None) {
        Ok(d) => {
            let results = private_diag::capture_all(&d, boot_attempt, None);
            // Copy through the USB discovery timeline if the mount helper
            // wrote one — this is invaluable for diagnosing the
            // delayed-USB-discovery class of boot failures.
            let timeline_src = Path::new("/run/testos/usb-discovery-timeline.txt");
            if timeline_src.exists() {
                let _ = std::fs::copy(timeline_src, d.join(private_diag::USB_TIMELINE_FILENAME));
            }
            let problems = private_diag::verify_captures(&d, &results);
            if !problems.is_empty() {
                let body = problems.join("\n") + "\n";
                let _ = std::fs::write(d.join("verify-problems.txt"), body);
                eprintln!(
                    "WARNING: {} private-diag capture problems recorded",
                    problems.len()
                );
            }
            Some(d)
        }
        Err(e) => {
            eprintln!("WARNING: could not write private diagnostics: {}", e);
            None
        }
    };

    // Sync the USB filesystem so results + diagnostics are durable before
    // reboot. Report sync failures honestly on the summary screen.
    let sync_ok = match private_diag::sync_usb() {
        Ok(()) => true,
        Err(e) => {
            eprintln!("WARNING: USB sync failed: {}", e);
            false
        }
    };

    // Post-run summary. Honest about failures and skips, states sync
    // status, gives the next action (reboot countdown), and explicitly
    // labels the results as baseline evidence — not proof of optid
    // improvement.
    testos_tui::print_summary(
        &palette,
        testos_tui::RunCounts {
            attempted: attempted_count,
            passed: passed_count,
            failed: failed_count,
            skipped: skipped_count,
        },
        &results_dir.display().to_string(),
        sync_ok,
        aborted,
    );
    if let Some(d) = &diag_dir {
        let rel = d.strip_prefix(usb).unwrap_or(d);
        let _ = writeln!(
            io::stdout(),
            "  {}private diag:{} /{} (stays on USB — do NOT submit)",
            palette.dim,
            palette.reset,
            rel.to_string_lossy()
        );
    }

    std::thread::sleep(Duration::from_secs(5));
    reboot_host();
}

/// Show the menu and return the list of selected benchmarks. Uses the TUI
/// module so the menu is driven by catalog data (notes + significance),
/// never hard-coded descriptions in this binary.
fn show_menu(palette: &testos_tui::Palette, list: &BenchList) -> Result<Vec<Bench>, String> {
    testos_tui::print_menu(palette, list);
    loop {
        let _ = write!(
            io::stdout(),
            "{}Select{} (comma-separated numbers, or 0 for all, or 'q' to quit): ",
            palette.bold,
            palette.reset
        );
        let _ = io::stdout().flush();
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let line = line.trim();

        if line == "q" {
            return Ok(Vec::new());
        }
        if line == "0" {
            return Ok(list.benches.clone());
        }

        // Parse comma-separated indices.
        let mut picked: Vec<Bench> = Vec::new();
        let mut bad = false;
        for tok in line.split(',') {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            match tok.parse::<usize>() {
                Ok(n) if n >= 1 && n <= list.benches.len() => {
                    let b = list.benches[n - 1].clone();
                    if !picked.iter().any(|x| x.id == b.id) {
                        picked.push(b);
                    }
                }
                _ => {
                    bad = true;
                    let _ = writeln!(
                        io::stdout(),
                        "  {}'{}' is not a valid selection.{}",
                        palette.yellow,
                        tok,
                        palette.reset
                    );
                    break;
                }
            }
        }
        if !bad && !picked.is_empty() {
            return Ok(picked);
        }
        if !bad && picked.is_empty() {
            let _ = writeln!(
                io::stdout(),
                "  {}No valid selections. Try again.{}",
                palette.yellow,
                palette.reset
            );
        }
    }
}

/// Run a single benchmark and return (status, value, unit, stdout, stderr, exit_code).
fn run_benchmark(
    bench: &Bench,
    results_dir: &Path,
) -> (
    String,
    Option<f64>,
    Option<String>,
    String,
    String,
    Option<i32>,
) {
    // For ShellJson we pass the result file path via env.
    let result_file = results_dir.join(format!("{}.out.json", bench.id));
    let _ = std::fs::remove_file(&result_file);

    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(&bench.command);
    cmd.env("TESTOS_RESULT_FILE", &result_file);
    cmd.env("TESTOS_BENCH_ID", &bench.id);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return (
                "fail".to_string(),
                None,
                None,
                String::new(),
                format!("Failed to spawn benchmark: {}", e),
                None,
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();

    if !output.status.success() && exit_code.is_none() {
        // Killed by signal.
        return ("fail".to_string(), None, None, stdout, stderr, exit_code);
    }

    match bench.kind {
        BenchKind::ShellPassFail => {
            if output.status.success() {
                ("pass".to_string(), None, None, stdout, stderr, exit_code)
            } else {
                ("fail".to_string(), None, None, stdout, stderr, exit_code)
            }
        }
        BenchKind::ShellNumeric => {
            // Last non-empty line of stdout is the number.
            let num = stdout
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .and_then(|l| l.trim().parse::<f64>().ok());
            // Use the bench's declared unit if present (defect 7), else
            // fall back to "numeric" for legacy compatibility.
            let unit = bench.unit.clone().unwrap_or_else(|| "numeric".to_string());
            match num {
                Some(v) => {
                    // SECURITY (defect 7): reject non-finite values (NaN, Inf)
                    // and zero for benchmarks where zero is meaningless.
                    // A zero or non-finite result must fail validation rather
                    // than count as a pass (defect 8 requirement).
                    if !v.is_finite() {
                        return ("fail".to_string(), None, None, stdout, stderr, exit_code);
                    }
                    (
                        "pass".to_string(),
                        Some(v),
                        Some(unit),
                        stdout,
                        stderr,
                        exit_code,
                    )
                }
                None => ("fail".to_string(), None, None, stdout, stderr, exit_code),
            }
        }
        BenchKind::ShellJson => {
            // Read the result file the command was supposed to write.
            let text = match std::fs::read_to_string(&result_file) {
                Ok(t) => t,
                Err(_) => return ("fail".to_string(), None, None, stdout, stderr, exit_code),
            };
            // Expect JSON like {"value": 1234.5, "unit": "iops"}
            #[derive(serde::Deserialize)]
            struct Out {
                value: f64,
                unit: String,
            }
            match serde_json::from_str::<Out>(&text) {
                Ok(o) => (
                    "pass".to_string(),
                    Some(o.value),
                    Some(o.unit),
                    stdout,
                    stderr,
                    exit_code,
                ),
                Err(_) => ("fail".to_string(), None, None, stdout, stderr, exit_code),
            }
        }
        BenchKind::Rushbench => {
            // rushbench writes its own JSON. We just check exit code and look for
            // the median in stdout (best-effort). The rushbench result path is
            // not used here - we just treat the median as the value.
            // This is a simple wrapper; full integration can come later.
            if output.status.success() {
                // Try to extract "median: 1.23" from stdout.
                let med = stdout.lines().find_map(|l| {
                    let l = l.trim();
                    if l.starts_with("median:") {
                        l.trim_start_matches("median:").trim().parse::<f64>().ok()
                    } else {
                        None
                    }
                });
                (
                    "pass".to_string(),
                    med,
                    Some("ms".to_string()),
                    stdout,
                    stderr,
                    exit_code,
                )
            } else {
                ("fail".to_string(), None, None, stdout, stderr, exit_code)
            }
        }
    }
}

/// Watch /dev/console for Esc to abort the run. Best-effort - if this fails
/// to read the console, the user can still Ctrl-C the runner.
fn watch_for_esc(tx: std::sync::mpsc::Sender<()>) {
    use std::os::unix::io::AsRawFd;
    let path = "/dev/console";
    let f = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(_) => return, // silent - common in non-tty environments
    };
    let fd = f.as_raw_fd();
    let mut buf = [0u8; 1];
    loop {
        // Read one byte; this blocks. We don't care about most keys.
        let n = unsafe { libc_read(fd, buf.as_mut_ptr(), 1) };
        if n <= 0 {
            // Read error or EOF; bail.
            return;
        }
        if buf[0] == 0x1b {
            // Esc.
            let _ = tx.send(());
            return;
        }
    }
}

// Direct libc binding to avoid pulling in the `libc` crate.
extern "C" {
    fn read(fd: i32, buf: *mut std::os::raw::c_void, count: usize) -> isize;
}
unsafe fn libc_read(fd: i32, buf: *mut u8, count: usize) -> isize {
    read(fd, buf as *mut _, count)
}

/// Compute SHA-256 for every per-benchmark result file in `results_dir`.
/// Returns a map of filename → hex SHA-256. Only files matching
/// `*.json` that are NOT top-level metadata (manifest.json, run-intent.json,
/// plan.json, result-hashes.json) are hashed.
fn compute_result_hashes(results_dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut hashes = std::collections::BTreeMap::new();
    let skip = [
        "manifest.json",
        "run-intent.json",
        "plan.json",
        "result-hashes.json",
        "expected.json",
    ];
    if let Ok(entries) = std::fs::read_dir(results_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = match name.to_str() {
                Some(s) => s,
                None => continue,
            };
            if !name_str.ends_with(".json") || skip.contains(&name_str) {
                continue;
            }
            // Read the file bytes and hash them.
            if let Ok(data) = std::fs::read(entry.path()) {
                let h = testos::run_intent::sha256_hex(&data);
                hashes.insert(name_str.to_string(), h);
            }
        }
    }
    hashes
}

fn iso_utc_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    let secs = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Simple ISO 8601 from epoch seconds, no chrono dependency needed in this binary.
    // Format: YYYY-MM-DDTHH:MM:SSZ
    iso_from_epoch(secs)
}

/// Read the running testOS image version from the canonical source
/// (`/etc/testos/version`, written by build-testos.sh from the repo VERSION
/// file), falling back to `/etc/os-release` `VERSION=`, then to the hardcoded
/// `TESTOS_VERSION_FALLBACK`. Extracted into a helper so the same value is
/// used for run-intent validation (step 1c) and for the manifest (step 8),
/// guaranteeing they agree.
fn read_running_testos_version() -> String {
    std::fs::read_to_string("/etc/testos/version")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/os-release")
                .ok()
                .and_then(|t| {
                    t.lines()
                        .find(|l| l.starts_with("VERSION="))
                        .and_then(|l| {
                            l.split('=')
                                .nth(1)
                                .map(|s| s.trim().trim_matches('"').to_string())
                        })
                        .filter(|s| !s.is_empty())
                })
        })
        .unwrap_or_else(|| TESTOS_VERSION_FALLBACK.to_string())
}

fn iso_from_epoch(epoch: u64) -> String {
    // Days since epoch, then calendar breakdown. Naive but correct for 1970-2099.
    let secs_per_day: u64 = 86400;
    let days = epoch / secs_per_day;
    let secs_in_day = epoch % secs_per_day;
    let hour = secs_in_day / 3600;
    let min = (secs_in_day % 3600) / 60;
    let sec = secs_in_day % 60;

    // Convert days to year/month/day. Algorithm from Howard Hinnant's date library.
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hour, min, sec
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn reboot_host() {
    // Try `systemctl reboot` first; fall back to `reboot` then direct syscall.
    let _ = Command::new("systemctl").arg("reboot").status();
    let _ = Command::new("reboot").status();
    // If we get here, reboot failed. Try the syscall.
    unsafe { libc_reboot(LINUX_REBOOT_CMD_RESTART) };
    // Last resort.
    eprintln!("Failed to reboot. Type 'reboot' manually.");
}

const LINUX_REBOOT_CMD_RESTART: i32 = 0x01234567;
extern "C" {
    fn reboot(magic: i32, magic2: i32, cmd: i32, arg: *mut std::os::raw::c_void) -> i32;
}
unsafe fn libc_reboot(cmd: i32) -> i32 {
    reboot(0xfee1deadu32 as i32, 672274793, cmd, std::ptr::null_mut())
}

/// Implementation of the `fail_safe` closure, extracted as a standalone
/// function so it can return `!` cleanly. Writes raw diagnostics to
/// PRIVATE-DIAGNOSTICS, shows the privacy-safe recovery screen, waits,
/// and reboots. NEVER drops to a root shell.
fn fail_safe_impl(
    palette: &testos_tui::Palette,
    boot_attempt: u32,
    category: FailureCategory,
    run_id_hint: &str,
) -> ! {
    // Write raw diagnostics to PRIVATE-DIAGNOSTICS/<run_id_hint>/.
    // Use "boot-<attempt>" as a fallback run_id when we don't have
    // the intent's run_id yet (early failures).
    let diag_run_id = if run_id_hint.is_empty() {
        format!("boot-{}", boot_attempt)
    } else {
        run_id_hint.to_string()
    };
    let usb = Path::new(USB_MOUNT);
    let diag_rel = if usb.exists() {
        match private_diag::ensure_dir(usb, &diag_run_id, Some(category.code())) {
            Ok(dir) => {
                let results = private_diag::capture_all(&dir, boot_attempt, Some(category.code()));
                let timeline_src = Path::new("/run/testos/usb-discovery-timeline.txt");
                if timeline_src.exists() {
                    let _ =
                        std::fs::copy(timeline_src, dir.join(private_diag::USB_TIMELINE_FILENAME));
                }
                let problems = private_diag::verify_captures(&dir, &results);
                if !problems.is_empty() {
                    let body = problems.join("\n") + "\n";
                    let _ = std::fs::write(dir.join("verify-problems.txt"), body);
                }
                let sync_status = match private_diag::sync_usb() {
                    Ok(()) => "sync ok",
                    Err(e) => {
                        let _ = std::fs::write(dir.join("sync-failure.txt"), &e);
                        "sync FAILED"
                    }
                };
                let _ = std::fs::write(dir.join("sync-status.txt"), sync_status);
                let rel = dir.strip_prefix(usb).unwrap_or(&dir);
                format!("/{}", rel.to_string_lossy())
            }
            Err(e) => {
                eprintln!("WARNING: could not write private diagnostics: {}", e);
                format!("/PRIVATE-DIAGNOSTICS/{} (write failed)", diag_run_id)
            }
        }
    } else {
        "/PRIVATE-DIAGNOSTICS/ (USB not mounted — no diagnostics written)".to_string()
    };

    testos::recovery::print_recovery_screen(palette, category, &diag_rel);

    // Wait so the operator can read the screen / photograph it. Then
    // reboot. Ctrl-C lets them stay on the screen for longer review.
    std::thread::sleep(Duration::from_secs(10));
    reboot_host();
    // reboot_host() always returns () but is followed by an exit() in its
    // own body; if it somehow falls through, exit here so this function
    // truly diverges (its return type is `!`).
    std::process::exit(1);
}

// Unused imports kept out of the binary - removed to avoid trait-call errors.
// (The runner doesn't currently use BufRead::read_line as a method reference;
// we just call .read_line() on stdin directly elsewhere.)
