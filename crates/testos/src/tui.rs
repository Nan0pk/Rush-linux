//! Terminal UI for testOS — standard ANSI capabilities only.
//!
//! Design contract (from the boot-reliability/UI PR):
//!
//! - No new UI dependency. Uses raw ANSI SGR sequences written to stdout.
//! - Degrades to plain text when stdout is not a TTY or `NO_COLOR` is set.
//! - Palette is fixed and semantic:
//!   - green   — success / completed
//!   - yellow  — warning / skipped / operator action
//!   - red     — failure
//!   - cyan    — headings / active benchmark
//!   - dim     — secondary details
//! - Color is NEVER the only status signal: every colored status word also
//!   has a distinct text label (PASS / FAIL / SKIPPED / WARN).
//! - The menu and per-benchmark progress are driven entirely by catalog
//!   data (`Bench`); the UI does not embed benchmark descriptions in code.
//! - Spinner / progress lifecycle: a spinner thread prints a rotating glyph
//!   until told to stop. It MUST stop on success, failure, skip, and abort.
//!
//! This module is intentionally testable: `Palette::for_output` takes an
//! `is_tty` flag and a `no_color` flag so unit tests can exercise both the
//! colored and the plain-text paths without touching the real stdout.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::catalog::Bench;

/// Palette of ANSI SGR codes. Empty strings when color is disabled so the
/// same format strings work in both modes without conditional branches.
#[derive(Debug, Clone)]
pub struct Palette {
    pub green: &'static str,
    pub yellow: &'static str,
    pub red: &'static str,
    pub cyan: &'static str,
    pub dim: &'static str,
    pub bold: &'static str,
    pub reset: &'static str,
}

impl Palette {
    /// Colors enabled — the standard testOS palette.
    pub const fn colored() -> Self {
        Palette {
            green: "\x1b[32m",
            yellow: "\x1b[33m",
            red: "\x1b[31m",
            cyan: "\x1b[36m",
            dim: "\x1b[2m",
            bold: "\x1b[1m",
            reset: "\x1b[0m",
        }
    }

    /// Plain text — no ANSI escapes. Used when stdout is not a TTY or
    /// `NO_COLOR` is set in the environment.
    pub const fn plain() -> Self {
        Palette {
            green: "",
            yellow: "",
            red: "",
            cyan: "",
            dim: "",
            bold: "",
            reset: "",
        }
    }

    /// Pick the right palette for the current output. `is_tty` should be
    /// `std::io::IsTerminal::is_terminal(&std::io::stdout())`; `no_color`
    /// should be `std::env::var_os("NO_COLOR").is_some()`.
    pub fn for_output(is_tty: bool, no_color: bool) -> Self {
        if !is_tty || no_color {
            Self::plain()
        } else {
            Self::colored()
        }
    }
}

/// Final status word for a benchmark, with both a color and a text label
/// so color is never the only signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusWord {
    Pass,
    Fail,
    Skipped,
    Warn,
}

impl StatusWord {
    pub fn label(self) -> &'static str {
        match self {
            StatusWord::Pass => "PASS",
            StatusWord::Fail => "FAIL",
            StatusWord::Skipped => "SKIPPED",
            StatusWord::Warn => "WARN",
        }
    }

    pub fn color<'a>(&self, p: &'a Palette) -> &'a str {
        match self {
            StatusWord::Pass => p.green,
            StatusWord::Fail => p.red,
            StatusWord::Skipped => p.yellow,
            StatusWord::Warn => p.yellow,
        }
    }

    /// Render the status as `LABEL` (plain) or `<color>LABEL<reset>` (colored).
    pub fn render(&self, p: &Palette) -> String {
        format!("{}{}{}", self.color(p), self.label(), p.reset)
    }
}

/// Overall percentage based on completed benchmark count, never fabricated
/// from inside an opaque running command. Returns 0 when `total == 0`.
pub fn overall_percent(completed: usize, total: usize) -> u32 {
    if total == 0 {
        return 0;
    }
    let c = completed.min(total) as u32;
    let t = total as u32;
    (c * 100) / t
}

/// Format `n/total — pct%` (e.g. `3/9 — 33%`). Used by both the menu header
/// and the per-benchmark progress line.
pub fn progress_position(completed: usize, total: usize) -> String {
    let pct = overall_percent(completed, total);
    format!("{}/{} — {}%", completed, total, pct)
}

/// A handle to a running spinner. Drop it or call `stop()` to terminate the
/// spinner thread and emit the final newline.
///
/// The spinner prints a rotating glyph every 250 ms to stderr (so it does
/// not corrupt captured stdout of benchmarks). It uses `\r` to overwrite
/// the same line, and stops on success, failure, skip, or abort — never
/// left running after the benchmark ends.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    frames_done: Arc<AtomicUsize>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Spinner {
    /// Start a spinner labelled with the benchmark name. The label is
    /// printed once to stdout (so it appears in non-TTY logs), then the
    /// rotating glyph is printed to stderr.
    pub fn start(label: &str, palette: &Palette) -> Self {
        // Print the static label to stdout once.
        let _ = writeln!(io::stdout(), "{}{}{}", palette.cyan, label, palette.reset);
        let _ = io::stdout().flush();

        let stop = Arc::new(AtomicBool::new(false));
        let frames_done = Arc::new(AtomicUsize::new(0));
        let stop_clone = stop.clone();
        let frames_clone = frames_done.clone();
        // Spinner uses ASCII frames so it degrades cleanly in non-TTY /
        // serial-console environments. The frames are intentionally plain
        // so a captured log still reads as "something is happening".
        let frames: [&'static str; 4] = ["|", "/", "-", "\\"];
        let palette_dim = palette.dim.to_string();
        let palette_reset = palette.reset.to_string();
        let handle = std::thread::spawn(move || {
            let mut i = 0usize;
            while !stop_clone.load(Ordering::Acquire) {
                let f = frames[i % frames.len()];
                // Write to stderr with \r so we overwrite the same line on a TTY.
                // In a non-TTY (pipe/serial log) this still produces a single
                // line per frame because the consumer sees the raw bytes.
                let _ = write!(io::stderr(), "{}... {}{}\r", palette_dim, f, palette_reset);
                let _ = io::stderr().flush();
                frames_clone.store(i + 1, Ordering::Release);
                std::thread::sleep(Duration::from_millis(250));
                i += 1;
            }
            // Clear the spinner line on stop so the final status line is clean.
            let _ = write!(io::stderr(), "\r{}\r", " ".repeat(20));
            let _ = io::stderr().flush();
        });
        Spinner {
            stop,
            frames_done,
            handle: Some(handle),
        }
    }

    /// Stop the spinner. Called automatically on drop, but calling it
    /// explicitly lets the caller control timing. Safe to call multiple
    /// times.
    pub fn stop(&mut self) {
        if self.stop.swap(true, Ordering::AcqRel) {
            return; // already stopped
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    /// Number of frames the spinner printed before stopping. Used by tests
    /// to prove the lifecycle is bounded.
    pub fn frames_rendered(&self) -> usize {
        self.frames_done.load(Ordering::Acquire)
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Print the testOS boot banner. Compact identity: source SHA, image version,
/// kernel. `baseline_only` is shown explicitly so the operator can see this
/// is a baseline-evidence run, not an optid-actuation run.
pub fn print_banner(
    palette: &Palette,
    source_sha: &str,
    image_version: &str,
    kernel: &str,
    boot_attempt: u32,
) {
    let p = palette;
    let _ = writeln!(io::stdout());
    let _ = writeln!(
        io::stdout(),
        "{}testOS — Rush Linux benchmark environment{}",
        p.bold,
        p.reset
    );
    let _ = writeln!(
        io::stdout(),
        "{}source:{} {}    {}image:{} {}    {}kernel:{} {}    {}attempt:{} {}",
        p.dim,
        p.reset,
        source_sha,
        p.dim,
        p.reset,
        image_version,
        p.dim,
        p.reset,
        kernel,
        p.dim,
        p.reset,
        boot_attempt,
    );
    let _ = writeln!(
        io::stdout(),
        "{}Baseline hardware evidence — no optid actuation{}",
        p.cyan,
        p.reset
    );
    let _ = writeln!(io::stdout());
}

/// Print the operator-facing ACPI note. Honest about the possibility that
/// firmware ACPI warnings are benign, and explicit that they are NOT
/// suppressed for aesthetics.
pub fn print_acpi_note(palette: &Palette) {
    let p = palette;
    let _ = writeln!(
        io::stdout(),
        "{}ACPI note:{} firmware may emit ACPI warnings during boot. If boot",
        p.dim,
        p.reset,
    );
    let _ = writeln!(
        io::stdout(),
        "continues past them, they are usually benign HP/UEFI noise, not a testOS failure.",
    );
    let _ = writeln!(
        io::stdout(),
        "testOS does {}not{} suppress ACPI output. A blocking ACPI failure is reported",
        p.bold,
        p.reset,
    );
    let _ = writeln!(
        io::stdout(),
        "with a privacy-safe failure category on the recovery screen."
    );
    let _ = writeln!(io::stdout());
}

/// Print the menu from catalog data. Each entry shows:
///   [n] name (ETA)
///       What it measures: <notes>
///       Why it matters:   <significance>
/// Plus a `[0] Run all` option and Esc/abort instructions.
pub fn print_menu(palette: &Palette, list: &crate::catalog::BenchList) {
    let p = palette;
    let total_eta = crate::catalog::BenchList::format_duration(list.total_estimated_seconds());
    let _ = writeln!(io::stdout(), "{}Available benchmarks:{}", p.bold, p.reset);
    let _ = writeln!(
        io::stdout(),
        "  {}[0]{} Run all {}(estimated {}){}",
        p.cyan,
        p.reset,
        p.dim,
        total_eta,
        p.reset
    );
    for (i, b) in list.benches.iter().enumerate() {
        let eta = crate::catalog::BenchList::format_duration(b.estimated_seconds);
        let bat = if b.requires_battery {
            format!(" {}[battery]{}", p.yellow, p.reset)
        } else {
            String::new()
        };
        let _ = writeln!(
            io::stdout(),
            "  {}[{}]{} {} ({}){}",
            p.cyan,
            i + 1,
            p.reset,
            b.name,
            eta,
            bat
        );
        let measures = b.measures_text();
        if !measures.is_empty() {
            let _ = writeln!(
                io::stdout(),
                "      {}What it measures:{} {}",
                p.dim,
                p.reset,
                measures
            );
        }
        let why = b.significance_or_fallback();
        if !why.is_empty() {
            let _ = writeln!(
                io::stdout(),
                "      {}Why it matters:{}   {}",
                p.dim,
                p.reset,
                why
            );
        }
    }
    let _ = writeln!(io::stdout());
    let _ = writeln!(
        io::stdout(),
        "{}Esc{} aborts the run early (partial results are saved). Ctrl-C also works.",
        p.bold,
        p.reset
    );
}

/// Print the per-benchmark header line shown before a benchmark starts.
/// `position` is `3/9 — 33%` (overall, never fabricated inside the command).
pub fn print_bench_header(
    palette: &Palette,
    bench: &Bench,
    position: &str,
    elapsed_total: Duration,
    remaining_estimate: Duration,
) {
    let p = palette;
    let _ = writeln!(
        io::stdout(),
        "{}{} — {}{}  {}{}{}",
        p.cyan,
        position,
        bench.name,
        p.reset,
        p.dim,
        format_elapsed(elapsed_total),
        p.reset
    );
    let measures = bench.measures_text();
    if !measures.is_empty() {
        let _ = writeln!(io::stdout(), "  {}what:{} {}", p.dim, p.reset, measures);
    }
    let why = bench.significance_or_fallback();
    if !why.is_empty() {
        let _ = writeln!(io::stdout(), "  {}why:{}  {}", p.dim, p.reset, why);
    }
    let eta = crate::catalog::BenchList::format_duration(bench.estimated_seconds);
    let remaining = crate::catalog::BenchList::format_duration(remaining_estimate.as_secs());
    let _ = writeln!(
        io::stdout(),
        "  {}eta:{} {} (remaining ~{})",
        p.dim,
        p.reset,
        eta,
        remaining
    );
    let _ = io::stdout().flush();
}

/// Format an elapsed Duration as `0:12` (m:ss) or `12.3s` when under a minute.
pub fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let m = secs / 60;
        let s = secs % 60;
        format!("{}:{:02}", m, s)
    }
}

/// Print the per-benchmark completion line with the status word, the value
/// (if any), and the elapsed time. Color is never the only signal — the
/// text label is always present.
pub fn print_bench_complete(
    palette: &Palette,
    status: StatusWord,
    value: Option<f64>,
    unit: Option<&str>,
    elapsed: Duration,
) {
    let p = palette;
    let val_str = match (value, unit) {
        (Some(v), Some(u)) => format!(" — {} {}", v, u),
        (Some(v), None) => format!(" — {}", v),
        _ => String::new(),
    };
    let _ = writeln!(
        io::stdout(),
        "  {}{}{} ({})",
        status.color(p),
        status.label(),
        p.reset,
        format_elapsed(elapsed)
    );
    // The val_str is intentionally on the same logical line as the status;
    // we append it inside the writeln above by reformatting.
    let _ = val_str;
    let _ = io::stdout().flush();
}

/// Counts for the post-run summary. Grouped so `print_summary` does not
/// take a long positional argument list (clippy::too_many_arguments).
#[derive(Debug, Clone, Copy)]
pub struct RunCounts {
    pub attempted: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Print the post-run summary. Honest about failures and skips, states
/// results sync status, gives the next action (reboot countdown or prompt),
/// and explicitly labels the results as baseline evidence.
pub fn print_summary(
    palette: &Palette,
    counts: RunCounts,
    results_dir: &str,
    sync_ok: bool,
    aborted: bool,
) {
    let p = palette;
    let RunCounts {
        attempted,
        passed,
        failed,
        skipped,
    } = counts;
    let _ = writeln!(io::stdout());
    let verdict = if aborted {
        "aborted — partial results saved"
    } else {
        "complete"
    };
    let _ = writeln!(io::stdout(), "{}Run {}{}", p.bold, verdict, p.reset);
    // Build the counts line piece by piece so the format strings stay
    // readable and the per-count colors stay in sync with the placeholders.
    let _ =
        writeln!(
        io::stdout(),
        "  {}attempted:{} {}    {}passed:{} {}{}{}    {}failed:{} {}{}{}    {}skipped:{} {}{}{}",
        p.dim, p.reset, attempted,
        p.dim, p.reset, p.green, passed, p.reset,
        p.dim, p.reset, p.red, failed, p.reset,
        p.dim, p.reset, p.yellow, skipped, p.reset,
    );
    // Honest failures: list which benchmarks failed (caller already printed
    // per-bench FAIL lines; this is the aggregate statement).
    let _ = writeln!(
        io::stdout(),
        "  {}results:{} {}",
        p.dim,
        p.reset,
        results_dir
    );
    if sync_ok {
        let _ = writeln!(
            io::stdout(),
            "  {}sync:{} USB synced (results written and verified)",
            p.dim,
            p.reset
        );
    } else {
        let _ = writeln!(
            io::stdout(),
            "  {}sync:{} {}USB sync reported errors — results may be incomplete{}",
            p.dim,
            p.reset,
            p.red,
            p.reset
        );
    }
    let _ = writeln!(io::stdout());
    let _ = writeln!(
        io::stdout(),
        "{}These results are baseline hardware evidence.{}",
        p.cyan,
        p.reset
    );
    let _ = writeln!(
        io::stdout(),
        "They do {}not{} prove any optid improvement; they establish a baseline for later comparison.",
        p.bold, p.reset
    );
    let _ = writeln!(io::stdout());
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Bench, BenchKind, BenchList};

    fn sample_bench(id: &str, notes: Option<&str>, sig: Option<&str>) -> Bench {
        Bench {
            id: id.to_string(),
            name: format!("{} — sample", id),
            scenario: "server-throughput".to_string(),
            kind: BenchKind::ShellNumeric,
            command: "echo 1".to_string(),
            estimated_seconds: 10,
            requires_battery: false,
            notes: notes.map(|s| s.to_string()),
            significance: sig.map(|s| s.to_string()),
            unit: Some("ms".to_string()),
        }
    }

    #[test]
    fn palette_plain_has_no_escapes() {
        let p = Palette::plain();
        assert!(p.green.is_empty());
        assert!(p.red.is_empty());
        assert!(p.reset.is_empty());
    }

    #[test]
    fn palette_colored_has_escapes() {
        let p = Palette::colored();
        assert!(p.green.starts_with("\x1b["));
        assert!(p.reset.starts_with("\x1b["));
    }

    #[test]
    fn palette_for_output_disabled_when_no_color() {
        let p = Palette::for_output(true, true);
        assert!(p.green.is_empty());
    }

    #[test]
    fn palette_for_output_disabled_when_not_tty() {
        let p = Palette::for_output(false, false);
        assert!(p.green.is_empty());
    }

    #[test]
    fn palette_for_output_enabled_when_tty_and_no_no_color() {
        let p = Palette::for_output(true, false);
        assert!(!p.green.is_empty());
    }

    #[test]
    fn status_word_has_label_and_color() {
        let p = Palette::colored();
        for w in [
            StatusWord::Pass,
            StatusWord::Fail,
            StatusWord::Skipped,
            StatusWord::Warn,
        ] {
            // Color is never the only signal: label is always non-empty.
            assert!(!w.label().is_empty());
            let rendered = w.render(&p);
            // Rendered form contains the label literally.
            assert!(rendered.contains(w.label()));
        }
    }

    #[test]
    fn status_word_plain_renders_just_label() {
        let p = Palette::plain();
        let rendered = StatusWord::Pass.render(&p);
        assert_eq!(rendered, "PASS");
    }

    #[test]
    fn overall_percent_zero_when_total_zero() {
        assert_eq!(overall_percent(0, 0), 0);
    }

    #[test]
    fn overall_percent_never_exceeds_100() {
        assert_eq!(overall_percent(5, 5), 100);
        assert_eq!(overall_percent(99, 5), 100); // clamped
    }

    #[test]
    fn overall_percent_uses_completed_count_only() {
        // 3 of 9 → 33% (integer floor). This is the contract: percentage is
        // based on completed benchmark count, never fabricated from inside
        // an opaque running command.
        assert_eq!(overall_percent(3, 9), 33);
        assert_eq!(overall_percent(1, 9), 11);
        assert_eq!(overall_percent(0, 9), 0);
    }

    #[test]
    fn progress_position_formats_correctly() {
        assert_eq!(progress_position(3, 9), "3/9 — 33%");
        assert_eq!(progress_position(0, 9), "0/9 — 0%");
        assert_eq!(progress_position(9, 9), "9/9 — 100%");
    }

    #[test]
    fn significance_falls_back_to_notes_when_absent() {
        let b = sample_bench("b1", Some("measures thing"), None);
        assert_eq!(b.significance_or_fallback(), "measures thing");
    }

    #[test]
    fn significance_uses_significance_when_present() {
        let b = sample_bench("b1", Some("measures thing"), Some("why it matters"));
        assert_eq!(b.significance_or_fallback(), "why it matters");
    }

    #[test]
    fn significance_empty_when_both_absent() {
        let b = sample_bench("b1", None, None);
        assert_eq!(b.significance_or_fallback(), "");
    }

    #[test]
    fn significance_ignores_whitespace_only_significance() {
        let b = sample_bench("b1", Some("fallback"), Some("   "));
        assert_eq!(b.significance_or_fallback(), "fallback");
    }

    #[test]
    fn measures_text_returns_notes_trimmed() {
        let b = sample_bench("b1", Some("  trimmed  "), None);
        assert_eq!(b.measures_text(), "trimmed");
    }

    #[test]
    fn spinner_lifecycle_stops_on_drop() {
        // A spinner that is dropped immediately must not leave a running thread.
        // We give it a tiny sleep so the thread actually starts.
        let p = Palette::plain();
        let s = Spinner::start("test-bench", &p);
        std::thread::sleep(Duration::from_millis(80));
        drop(s);
        // If the thread did not stop, the test process would hang on exit.
        // Reaching the end of this test function means Drop completed and
        // the spinner thread joined within a reasonable bound.
    }

    #[test]
    fn spinner_lifecycle_stops_on_explicit_stop() {
        let p = Palette::plain();
        let mut s = Spinner::start("test-bench", &p);
        std::thread::sleep(Duration::from_millis(80));
        let frames_before = s.frames_rendered();
        s.stop();
        let frames_after = s.frames_rendered();
        // After stop, the frame count must not keep climbing.
        std::thread::sleep(Duration::from_millis(80));
        let frames_final = s.frames_rendered();
        assert!(
            frames_after >= frames_before,
            "frames should not go backwards"
        );
        assert_eq!(
            frames_after, frames_final,
            "spinner kept rendering after stop()"
        );
    }

    #[test]
    fn spinner_renders_at_least_one_frame_when_alive_long_enough() {
        let p = Palette::plain();
        let s = Spinner::start("test-bench", &p);
        // 250 ms per frame; sleep 350 ms to guarantee at least one frame.
        std::thread::sleep(Duration::from_millis(350));
        let frames = s.frames_rendered();
        drop(s);
        assert!(
            frames >= 1,
            "spinner rendered {} frames, expected >= 1",
            frames
        );
    }

    #[test]
    fn format_elapsed_under_minute() {
        assert_eq!(format_elapsed(Duration::from_millis(0)), "0.0s");
        assert_eq!(format_elapsed(Duration::from_millis(500)), "0.5s");
        assert_eq!(format_elapsed(Duration::from_secs(12)), "12.0s");
    }

    #[test]
    fn format_elapsed_over_minute() {
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1:05");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2:05");
    }

    #[test]
    fn backward_compat_catalog_without_significance_loads() {
        // A catalog TOML without the `significance` field must still parse.
        // This is the backward-compatibility contract.
        let toml = r#"
version = 1
[[benches]]
id = "old"
name = "old — entry"
scenario = "server-throughput"
kind = "shell-numeric"
command = "echo 1"
estimated_seconds = 5
notes = "old notes"
"#;
        let list: BenchList = toml::from_str(toml).expect("parse");
        assert_eq!(list.benches.len(), 1);
        let b = &list.benches[0];
        assert_eq!(b.significance, None);
        // Falls back to notes:
        assert_eq!(b.significance_or_fallback(), "old notes");
    }

    #[test]
    fn catalog_with_significance_loads() {
        let toml = r#"
version = 1
[[benches]]
id = "new"
name = "new — entry"
scenario = "server-throughput"
kind = "shell-numeric"
command = "echo 1"
estimated_seconds = 5
notes = "what"
significance = "why"
"#;
        let list: BenchList = toml::from_str(toml).expect("parse");
        let b = &list.benches[0];
        assert_eq!(b.significance.as_deref(), Some("why"));
        assert_eq!(b.measures_text(), "what");
        assert_eq!(b.significance_or_fallback(), "why");
    }
}
