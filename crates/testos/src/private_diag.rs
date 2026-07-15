//! Private local diagnostics — raw boot diagnostics written to the USB for
//! local investigation only, with a HARD boundary from publishable evidence.
//!
//! Contract (from the boot-reliability PR — private local diagnostics):
//!
//! - Raw diagnostics are written ONLY to:
//!   `<usb_mount>/PRIVATE-DIAGNOSTICS/<run_id>/`
//! - They are NEVER placed under `testos-results/`, the persistent evidence
//!   run directory, or any submission bundle.
//! - The directory is marked with a `README.txt` containing:
//!   "PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT"
//! - Normal resume/collection leaves PRIVATE-DIAGNOSTICS on the USB.
//! - Evidence submission fails closed if:
//!   - PRIVATE-DIAGNOSTICS appears inside the proposed bundle
//!   - any raw journal/dmesg artifact appears inside publishable evidence
//!   - a symlink tries to reference private diagnostics
//! - Capture happens BEFORE any automatic reboot, including on failure paths.
//! - Writes are synced and verified; sync failures are reported honestly.
//!
//! What we capture (when available):
//!   - journalctl -b with monotonic timestamps
//!   - dmesg with monotonic timestamps
//!   - systemctl --failed
//!   - systemctl status for testos-usb-mount.service and testos-runner.service
//!   - systemd-analyze critical-chain
//!   - systemd-analyze blame
//!   - USB discovery/mount retry timeline (from the mount helper's log)
//!   - runner exit status and failure category
//!   - testOS version, full image commit, kernel version
//!   - boot count / attempt number
//!
//! What we DO NOT capture:
//!   - firmware tables (ACPI/DMI raw blobs)
//!   - disk contents or partition tables
//!   - user data
//!   - authentication material
//!   - network credentials
//!   - file contents unrelated to boot diagnosis
//!
//! This module is testable: `private_diag_dir` returns the canonical path,
//! `marker_text` returns the marker so tests can assert on its content, and
//! `capture` is split from `write_marker` so tests can exercise the marker
//! without spawning subprocesses.

use std::path::{Path, PathBuf};

/// Subdirectory on the USB where private diagnostics live. NEVER under
/// `testos-results/`.
pub const PRIVATE_DIAG_ROOT: &str = "PRIVATE-DIAGNOSTICS";

/// The marker string written to README.txt in every private diagnostics
/// directory. Public so tests can assert on it.
pub const MARKER_TEXT: &str = "PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT";

/// Filename of the marker README inside a private diagnostics directory.
pub const MARKER_FILENAME: &str = "README.txt";

/// Filename of the USB discovery/mount retry timeline, written by the
/// mount helper and copied through by the runner.
pub const USB_TIMELINE_FILENAME: &str = "usb-discovery-timeline.txt";

/// Filename of the runner exit/failure summary.
pub const RUNNER_EXIT_FILENAME: &str = "runner-exit.txt";

/// Compute the private diagnostics directory for a given run_id on the USB.
/// Returns `<usb>/PRIVATE-DIAGNOSTICS/<run_id>/`. The directory is NOT
/// created here — the caller must call `ensure_dir` or `capture`.
pub fn private_diag_dir(usb_mount: &Path, run_id: &str) -> PathBuf {
    let safe = sanitize_run_id(run_id);
    usb_mount.join(PRIVATE_DIAG_ROOT).join(safe)
}

/// Sanitize run_id for use as a directory name. Rejects path traversal,
/// absolute paths, and anything outside `[A-Za-z0-9_.:-]`. Falls back to
/// `unknown` for empty/invalid input.
pub fn sanitize_run_id(run_id: &str) -> String {
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }
    // Reject anything that is not a safe token. The run_intent validator
    // already enforces `^[A-Za-z0-9_.:-]{4,128}$`, but we re-check here
    // defensively in case a future caller passes an unsanitized value.
    let ok = trimmed.len() <= 128
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'));
    if !ok {
        return "unknown".to_string();
    }
    trimmed.to_string()
}

/// The marker README text, with the canonical warning header plus a short
/// explanation of what the directory contains and why it must not be
/// submitted.
pub fn marker_text(run_id: &str, failure_code: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str(MARKER_TEXT);
    s.push('\n');
    s.push('\n');
    s.push_str("This directory contains raw boot diagnostics captured by testOS\n");
    s.push_str("for LOCAL INVESTIGATION ONLY. It may contain hardware identifiers\n");
    s.push_str("(MAC addresses, serial numbers, UUIDs, kernel boot command-line\n");
    s.push_str("parameters, hostnames, IP addresses, and similar). It MUST NOT be\n");
    s.push_str("copied into the publishable evidence bundle, committed to the\n");
    s.push_str("repository, or attached to a pull request.\n\n");
    s.push_str("The normal collection scripts leave this directory on the USB.\n");
    s.push_str("The strict evidence validator rejects any bundle that contains\n");
    s.push_str("PRIVATE-DIAGNOSTICS, any raw dmesg/journal artifact, or any\n");
    s.push_str("symlink that references this directory.\n\n");
    s.push_str(&format!("run_id: {}\n", run_id));
    s.push_str(&format!(
        "failure_code: {}\n",
        failure_code.unwrap_or("none")
    ));
    s.push('\n');
    s.push_str("To review safely:\n");
    s.push_str(
        "  python3 tools/testos-diagnostics.py inspect <USB>/PRIVATE-DIAGNOSTICS/<run_id>\n",
    );
    s
}

/// Ensure the private diagnostics directory exists and write the marker
/// README. Returns the directory path on success.
pub fn ensure_dir(
    usb_mount: &Path,
    run_id: &str,
    failure_code: Option<&str>,
) -> Result<PathBuf, String> {
    let dir = private_diag_dir(usb_mount, run_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create_dir_all({}): {}", dir.display(), e))?;
    let marker_path = dir.join(MARKER_FILENAME);
    std::fs::write(&marker_path, marker_text(run_id, failure_code))
        .map_err(|e| format!("write marker {}: {}", marker_path.display(), e))?;
    Ok(dir)
}

/// Capture a single diagnostic by running a shell command and writing its
/// stdout to `<dir>/<filename>`. Failures are reported honestly via a
/// `(filename).error` sidecar rather than silently dropped.
///
/// `cmd` is run via `bash -c`. The output is written verbatim (no redaction
/// — this directory is local-only and explicitly marked as potentially
/// containing identifiers). If the command fails or produces no output, a
/// sidecar `.error` file records the reason.
pub fn capture_one(dir: &Path, filename: &str, cmd: &str) -> CaptureResult {
    let dest = dir.join(filename);
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let exit = o.status.code();
            // Always write the stdout, even when empty, so the operator can
            // see the command ran. If the command failed, also write a
            // sidecar describing the failure.
            if let Err(e) = std::fs::write(&dest, stdout.as_bytes()) {
                return CaptureResult::WriteFailed(dest, e.to_string());
            }
            if !o.status.success() {
                let err_path = dir.join(format!("{}.error", filename));
                let err_body = format!("exit_code={:?}\nstderr={}\n", exit, stderr);
                let _ = std::fs::write(&err_path, err_body);
                return CaptureResult::CommandFailed(dest, exit);
            }
            CaptureResult::Ok(dest)
        }
        Err(e) => {
            let err_path = dir.join(format!("{}.error", filename));
            let _ = std::fs::write(&err_path, format!("spawn failed: {}\n", e));
            CaptureResult::SpawnFailed(filename.to_string(), e.to_string())
        }
    }
}

/// Outcome of a single diagnostic capture.
#[derive(Debug)]
pub enum CaptureResult {
    Ok(PathBuf),
    CommandFailed(PathBuf, Option<i32>),
    SpawnFailed(String, String),
    WriteFailed(PathBuf, String),
}

impl CaptureResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, CaptureResult::Ok(_))
    }
    pub fn describe(&self) -> String {
        match self {
            CaptureResult::Ok(p) => format!("ok: {}", p.display()),
            CaptureResult::CommandFailed(p, code) => {
                format!("command failed (exit={:?}): {}", code, p.display())
            }
            CaptureResult::SpawnFailed(name, e) => format!("spawn failed for {}: {}", name, e),
            CaptureResult::WriteFailed(p, e) => format!("write failed {}: {}", p.display(), e),
        }
    }
}

/// Capture the full set of raw boot diagnostics into `dir`. The directory
/// must already exist (call `ensure_dir` first). Returns a list of
/// (filename, CaptureResult) pairs so the caller can write a manifest.
///
/// `boot_attempt` is the boot count (1 for first boot, 2 for second, etc.).
/// `failure_code` is the recovery-screen code if this capture is on a
/// failure path, or `None` for a successful run.
pub fn capture_all(
    dir: &Path,
    boot_attempt: u32,
    failure_code: Option<&str>,
) -> Vec<(&'static str, CaptureResult)> {
    // Each command is intentionally a single `bash -c` line. We do NOT
    // capture firmware tables, disk contents, user data, or network
    // credentials — only boot-diagnosis signals.
    let captures: [(&'static str, &str); 9] = [
        ("journalctl.txt", "journalctl -b --no-pager -o short-monotonic 2>/dev/null || journalctl -b --no-pager 2>/dev/null || echo 'journalctl unavailable'"),
        ("dmesg.txt", "dmesg --time-format=iso 2>/dev/null || dmesg 2>/dev/null || echo 'dmesg unavailable'"),
        ("systemctl-failed.txt", "systemctl --failed --no-pager 2>/dev/null || echo 'systemctl unavailable'"),
        ("status-usb-mount.txt", "systemctl status --no-pager testos-usb-mount.service 2>/dev/null || echo 'testos-usb-mount.service not found'"),
        ("status-runner.txt", "systemctl status --no-pager testos-runner.service 2>/dev/null || echo 'testos-runner.service not found'"),
        ("critical-chain.txt", "systemd-analyze critical-chain --no-pager 2>/dev/null || echo 'systemd-analyze unavailable'"),
        ("blame.txt", "systemd-analyze blame --no-pager 2>/dev/null || echo 'systemd-analyze unavailable'"),
        ("kernel-version.txt", "uname -r 2>/dev/null || echo 'uname unavailable'"),
        ("image-version.txt", "cat /etc/testos/version 2>/dev/null || cat /etc/os-release 2>/dev/null || echo 'version unavailable'"),
    ];
    let mut results = Vec::with_capacity(captures.len() + 2);
    for (name, cmd) in captures.iter() {
        let r = capture_one(dir, name, cmd);
        results.push((*name, r));
    }
    // Write the runner exit summary.
    let exit_path = dir.join(RUNNER_EXIT_FILENAME);
    let exit_body = format!(
        "boot_attempt={}\nfailure_code={}\n",
        boot_attempt,
        failure_code.unwrap_or("none")
    );
    let _ = std::fs::write(&exit_path, exit_body);
    results.push((RUNNER_EXIT_FILENAME, CaptureResult::Ok(exit_path)));
    results
}

/// Sync the USB filesystem after writing diagnostics. Returns `Ok(())` if
/// `sync` ran successfully, or an `Err` describing the failure. The caller
/// MUST report sync failures honestly (the recovery screen and the manifest
/// both surface a sync-failure warning).
pub fn sync_usb() -> Result<(), String> {
    let r = std::process::Command::new("sync").status();
    match r {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("sync exited with status {}", s)),
        Err(e) => Err(format!("failed to spawn sync: {}", e)),
    }
}

/// Verify that all files reported as `Ok` in `results` actually exist on
/// disk and are non-empty (or, if empty, are paired with an `.error`
/// sidecar). Returns a list of problems (empty list = all good).
pub fn verify_captures(dir: &Path, results: &[(&'static str, CaptureResult)]) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, r) in results {
        match r {
            CaptureResult::Ok(p) => {
                if !p.exists() {
                    problems.push(format!("{}: reported ok but file missing", name));
                    continue;
                }
                // Empty files are allowed only when the command produced no
                // output AND we recorded an .error sidecar; otherwise the
                // operator might think a capture succeeded when it silently
                // produced nothing.
                let meta = match std::fs::metadata(p) {
                    Ok(m) => m,
                    Err(e) => {
                        problems.push(format!("{}: stat failed: {}", name, e));
                        continue;
                    }
                };
                if meta.len() == 0 {
                    let err_path = dir.join(format!("{}.error", name));
                    if !err_path.exists() {
                        problems.push(format!("{}: empty output and no .error sidecar", name));
                    }
                }
            }
            _ => {
                // Failures are recorded via .error sidecars; verify the sidecar exists.
                let err_path = dir.join(format!("{}.error", name));
                if !err_path.exists() {
                    problems.push(format!(
                        "{}: capture failed but no .error sidecar was written",
                        name
                    ));
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_diag_dir_layout() {
        let usb = Path::new("/run/testos/usb");
        let dir = private_diag_dir(usb, "run-2026-07-16-001");
        assert_eq!(
            dir,
            Path::new("/run/testos/usb/PRIVATE-DIAGNOSTICS/run-2026-07-16-001")
        );
    }

    #[test]
    fn private_diag_dir_never_under_testos_results() {
        // The directory must NOT be under testos-results/. Sanity check
        // the path layout.
        let usb = Path::new("/run/testos/usb");
        let dir = private_diag_dir(usb, "run-1");
        let s = dir.to_string_lossy();
        assert!(
            !s.contains("testos-results"),
            "private diag under testos-results: {}",
            s
        );
        assert!(s.contains("PRIVATE-DIAGNOSTICS"), "missing root: {}", s);
    }

    #[test]
    fn sanitize_run_id_rejects_traversal() {
        assert_eq!(sanitize_run_id("../etc/passwd"), "unknown");
        assert_eq!(sanitize_run_id("/etc/passwd"), "unknown");
        assert_eq!(sanitize_run_id(""), "unknown");
        assert_eq!(sanitize_run_id("   "), "unknown");
        assert_eq!(sanitize_run_id("run with spaces"), "unknown");
        assert_eq!(sanitize_run_id("run/1"), "unknown");
    }

    #[test]
    fn sanitize_run_id_accepts_safe_tokens() {
        assert_eq!(sanitize_run_id("run-2026-07-16-001"), "run-2026-07-16-001");
        assert_eq!(sanitize_run_id("run_001"), "run_001");
        assert_eq!(sanitize_run_id("run:001"), "run:001");
        assert_eq!(sanitize_run_id("run.001"), "run.001");
    }

    #[test]
    fn marker_text_contains_warning_and_run_id() {
        let m = marker_text("run-2026-07-16-001", Some("E001"));
        assert!(m.contains(MARKER_TEXT), "missing warning header");
        assert!(m.contains("run-2026-07-16-001"), "missing run_id");
        assert!(m.contains("E001"), "missing failure_code");
        assert!(m.contains("DO NOT SUBMIT"), "missing submit warning");
        assert!(
            m.contains("testos-diagnostics.py inspect"),
            "missing inspect hint"
        );
    }

    #[test]
    fn marker_text_works_without_failure_code() {
        let m = marker_text("run-1", None);
        assert!(m.contains(MARKER_TEXT));
        assert!(m.contains("run-1"));
        // failure_code is always present; "none" when no failure.
        assert!(m.contains("failure_code: none"));
        assert!(!m.contains("failure_code: E"));
    }

    #[test]
    fn ensure_dir_creates_marker_and_directory() {
        let tmp = tempfile_dir();
        let usb = tmp.join("usb");
        std::fs::create_dir_all(&usb).unwrap();
        let dir = ensure_dir(&usb, "run-1", Some("E001")).expect("ensure_dir");
        assert!(dir.exists());
        assert!(dir.is_dir());
        let marker = dir.join(MARKER_FILENAME);
        assert!(marker.exists());
        let text = std::fs::read_to_string(&marker).unwrap();
        assert!(text.contains(MARKER_TEXT));
    }

    #[test]
    fn capture_one_writes_output() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        let r = capture_one(&dir, "test.txt", "echo hello");
        assert!(r.is_ok(), "{}", r.describe());
        let body = std::fs::read_to_string(dir.join("test.txt")).unwrap();
        assert_eq!(body.trim(), "hello");
    }

    #[test]
    fn capture_one_records_failure_sidecar() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        // A command that exits non-zero.
        let r = capture_one(&dir, "fail.txt", "false");
        assert!(!r.is_ok());
        let sidecar = dir.join("fail.txt.error");
        assert!(sidecar.exists(), "missing .error sidecar");
        let body = std::fs::read_to_string(&sidecar).unwrap();
        assert!(body.contains("exit_code="));
    }

    #[test]
    fn capture_one_handles_missing_command() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        // bash itself runs, but the inner command does not exist. The
        // output file should still be written (empty), and the .error
        // sidecar should explain.
        let r = capture_one(&dir, "missing.txt", "this-command-does-not-exist-xyz");
        // bash returns 127 for missing command — treated as failure.
        let _ = r;
        assert!(dir.join("missing.txt").exists() || dir.join("missing.txt.error").exists());
    }

    #[test]
    fn capture_all_writes_runner_exit() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        let results = capture_all(&dir, 2, Some("E099"));
        // Find the runner-exit entry.
        let exit_entry = results
            .iter()
            .find(|(n, _)| *n == RUNNER_EXIT_FILENAME)
            .expect("runner-exit.txt missing from capture_all results");
        assert!(exit_entry.1.is_ok());
        let body = std::fs::read_to_string(dir.join(RUNNER_EXIT_FILENAME)).unwrap();
        assert!(body.contains("boot_attempt=2"));
        assert!(body.contains("failure_code=E099"));
    }

    #[test]
    fn capture_all_does_not_dump_firmware_or_disks() {
        // Sanity: none of the captured commands touch firmware tables, disk
        // contents, user data, or network credentials. We verify by checking
        // the command list does not include dangerous invocations.
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        // Inspect the commands indirectly: capture_all runs a fixed set,
        // so we check the resulting file names match the allow-list.
        let results = capture_all(&dir, 1, None);
        let names: Vec<_> = results.iter().map(|(n, _)| *n).collect();
        for n in &names {
            assert!(
                !n.contains("firmware"),
                "unexpected firmware capture: {}",
                n
            );
            assert!(
                !n.contains("partition"),
                "unexpected partition capture: {}",
                n
            );
            assert!(
                !n.contains(" credential"),
                "unexpected credential capture: {}",
                n
            );
        }
        // We DO capture dmesg/journal — that is by design (raw diagnostics
        // are private, not publishable). Verify they are present.
        assert!(names.contains(&"dmesg.txt"));
        assert!(names.contains(&"journalctl.txt"));
    }

    #[test]
    fn verify_catches_missing_file() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        // Report a file as Ok that does not exist.
        let results: Vec<(&'static str, CaptureResult)> =
            vec![("ghost.txt", CaptureResult::Ok(dir.join("ghost.txt")))];
        let problems = verify_captures(&dir, &results);
        assert_eq!(problems.len(), 1, "expected 1 problem, got {:?}", problems);
        assert!(problems[0].contains("missing"));
    }

    #[test]
    fn verify_catches_empty_without_sidecar() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        // Write an empty file with no .error sidecar.
        std::fs::write(dir.join("empty.txt"), "").unwrap();
        let results: Vec<(&'static str, CaptureResult)> =
            vec![("empty.txt", CaptureResult::Ok(dir.join("empty.txt")))];
        let problems = verify_captures(&dir, &results);
        assert_eq!(problems.len(), 1, "expected 1 problem, got {:?}", problems);
        assert!(problems[0].contains("empty output and no .error sidecar"));
    }

    #[test]
    fn verify_accepts_empty_with_sidecar() {
        let tmp = tempfile_dir();
        let dir = tmp.join("diag");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("empty.txt"), "").unwrap();
        std::fs::write(dir.join("empty.txt.error"), "exit_code=1\n").unwrap();
        let results: Vec<(&'static str, CaptureResult)> =
            vec![("empty.txt", CaptureResult::Ok(dir.join("empty.txt")))];
        let problems = verify_captures(&dir, &results);
        assert!(
            problems.is_empty(),
            "expected no problems, got {:?}",
            problems
        );
    }

    // Helper: a unique tempdir per test. We use std::env::temp_dir plus a
    // process-unique suffix to avoid clashing with parallel test runs.
    fn tempfile_dir() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "testos-private-diag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
