//! `mixed-load-001` — the v0.6 Phase D comparison workload.
//!
//! Defined by `docs/strategy/mixed-load-workload.md` (D2). One cycle walks five
//! phases so every load-producing workload class appears in a single run and
//! each per-phase delta is attributable to one class:
//!
//! | # | Phase | Duration | Expected class |
//! |---|-------|----------|----------------|
//! | 1 | idle-warm | 60 s | `idle` |
//! | 2 | interactive | 60 s | `interactive` |
//! | 3 | throughput | 60 s | `throughput` |
//! | 4 | latency-critical | 60 s | `latency-critical` |
//! | 5 | idle-cooldown | 30 s | `idle` |
//!
//! The preset deliberately does **not** pin the class with `optctl pin` the way
//! `run_cell` does for three of its four load-producing phases: observing
//! whether the classifier reaches the expected class under real load is part
//! of the evidence. A mismatch is recorded as a `class_mismatch:<observed>`
//! anomaly on that phase's records, never as a hard error. A baseline arm has
//! no `optid` at all, so `class_observed` is `unmeasured` with an
//! `optid_absent` anomaly.
//!
//! **Phase 4 is the one exception, and it is asserted, not observed.**
//! `policy.rs`'s `LatencyCritical` branch requires `on_ac == Some(true)`, and
//! Criterion 3 requires the full cycle to run on battery — the class is
//! unreachable there by construction, independent of what `glmark2` does (see
//! `docs/inbox/2026-08-22-phase-d-latency-critical-blocked.md`, decision A).
//! Phase 4's driver is therefore wrapped with `gamemoderun` — the same
//! `com.feralinteractive.GameMode` D-Bus path Steam and Lutris already use in
//! production, already implemented and tested in
//! `crates/optid/src/shim/gamemode.rs` — so the class comes from a real
//! `RegisterGame` call against the actual `glmark2` process, not a policy
//! change or a synthetic override. If `gamemoderun` is absent, this phase
//! silently reverts to observation and the pre-existing `class_mismatch`
//! anomaly applies; if it is present and the class still fails to land, that
//! is recorded as `class_pin_ineffective:<observed>` instead, because it now
//! means the assertion path itself is broken, not that inference failed.
//!
//! ## Sample units
//!
//! `RunRecord.samples` holds integers in whatever unit the probe emits. The
//! pre-existing convention is that fractional metrics are recorded in
//! milli-units (`psi-*-avg10` is stored ×1000), and this preset follows it:
//! `frametime-*-ms` samples are microseconds, `discharge-w` samples are
//! milliwatts, and `joules-per-work-unit` samples are millijoules per unit.
//! `foreground-launch-ms` keeps its existing whole-millisecond samples.
//! `results.csv` always prints the median in the metric's *declared* unit, so
//! the human-facing artifact needs no scaling knowledge.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::contracts::{get_optctl_status_json, parse_contracts_toml, parse_optctl_status};
use crate::energy::{calculate_window, read_on_ac, EnergySample, EnergySource};
use crate::probes::{run_probe_for_metric, ProbeResult};
use crate::runner::build_record;
use crate::types::EnergyInfo;
use crate::utils::{
    find_repo_file, get_cpu_model, get_git_sha, get_kernel_version, get_utc_timestamp, percentile,
};

/// The only preset this harness defines. Named in
/// `docs/strategy/mixed-load-workload.md` and in the host-bench evidence
/// template's D3/D4 commands.
pub const MIXED_LOAD_001: &str = "mixed-load-001";

/// Cycles per tagged run. The workload spec requires N = 5; records with
/// `n < 5` carry an `insufficient_n` anomaly and are not milestone evidence.
pub const REQUIRED_CYCLES: usize = 5;

/// What drives a phase's load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Driver {
    /// Nothing runs; the phase measures the settled machine.
    Quiescent,
    /// Firefox rendering a fixed local JavaScript benchmark page.
    Interactive,
    /// `ninja` rebuilding a deterministic C++ project.
    Throughput,
    /// `glmark2 --run-forever --fullscreen`, instrumented by MangoHud's
    /// per-frame log so the frametime percentiles are real percentiles.
    LatencyCritical,
}

/// One phase of one cycle.
#[derive(Clone, Debug)]
pub struct PhaseSpec {
    pub name: &'static str,
    pub duration: Duration,
    pub expected_class: &'static str,
    pub driver: Driver,
    pub metrics: &'static [&'static str],
}

/// Divisor applied to every phase duration, from `RUSHBENCH_PHASE_SCALE`. It
/// exists so the sequencer itself can be exercised in seconds instead of half
/// an hour; any value other than 1 shortens the measurement windows and the run
/// is stamped `phase_scale_shortened` so it can never be mistaken for evidence.
pub fn phase_scale() -> u32 {
    std::env::var("RUSHBENCH_PHASE_SCALE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1)
}

/// A phase duration divided by the scale, never shorter than one second.
pub fn scaled_duration(seconds: u64, scale: u64) -> Duration {
    Duration::from_secs((seconds / scale.max(1)).max(1))
}

/// The `mixed-load-001` phase sequence, in order. Durations are the spec's,
/// divided by [`phase_scale`].
pub fn preset_phases(name: &str) -> Result<Vec<PhaseSpec>, String> {
    if name != MIXED_LOAD_001 {
        return Err(format!(
            "unknown preset: {name} (this harness defines only {MIXED_LOAD_001})"
        ));
    }
    let scale = u64::from(phase_scale());
    let scaled = |seconds: u64| scaled_duration(seconds, scale);
    Ok(vec![
        PhaseSpec {
            name: "idle-warm",
            duration: scaled(60),
            expected_class: "idle",
            driver: Driver::Quiescent,
            metrics: &["psi-cpu-avg10", "discharge-w"],
        },
        PhaseSpec {
            name: "interactive",
            duration: scaled(60),
            expected_class: "interactive",
            driver: Driver::Interactive,
            metrics: &[
                "input-latency-p95-ms",
                "input-latency-p99-ms",
                "foreground-launch-ms",
                "psi-cpu-avg10",
            ],
        },
        PhaseSpec {
            name: "throughput",
            duration: scaled(60),
            expected_class: "throughput",
            driver: Driver::Throughput,
            metrics: &["psi-cpu-avg10", "psi-io-avg10", "joules-per-work-unit"],
        },
        PhaseSpec {
            name: "latency-critical",
            duration: scaled(60),
            expected_class: "latency-critical",
            driver: Driver::LatencyCritical,
            metrics: &["frametime-p95-ms", "frametime-p99-ms", "psi-cpu-avg10"],
        },
        PhaseSpec {
            name: "idle-cooldown",
            duration: scaled(30),
            expected_class: "idle",
            driver: Driver::Quiescent,
            metrics: &["psi-cpu-avg10", "discharge-w"],
        },
    ])
}

/// The unit each metric's `results.csv` median is printed in, and the divisor
/// that converts a stored sample into it.
pub fn metric_unit(metric: &str) -> (&'static str, f64) {
    match metric {
        "psi-cpu-avg10" | "psi-io-avg10" => ("percent", 1000.0),
        "frametime-p95-ms" | "frametime-p99-ms" => ("ms", 1000.0),
        "input-latency-p95-ms" | "input-latency-p99-ms" => ("ms", 1000.0),
        "discharge-w" => ("W", 1000.0),
        "joules-per-work-unit" => ("J", 1000.0),
        "foreground-launch-ms" => ("ms", 1.0),
        _ => ("raw", 1.0),
    }
}

// ── driver outcomes ──────────────────────────────────────────────────────────

/// What a driver produced over one phase window.
#[derive(Default, Debug)]
pub struct DriverOutcome {
    /// Per-frame times in microseconds (`LatencyCritical` only).
    pub frametimes_us: Vec<u64>,
    /// Completed build edges (`Throughput` only).
    pub work_units: Option<u64>,
    /// Reasons a metric could not be produced on this host.
    pub unsupported: Vec<String>,
    /// `true` when this phase's driver was wrapped with `gamemoderun`, i.e.
    /// its class is asserted via `RegisterGame` rather than left for the
    /// classifier to infer. Only ever set for `Driver::LatencyCritical`.
    pub pinned_via_gamemode: bool,
}

/// A running driver plus everything needed to stop it and harvest its output.
struct RunningDriver {
    child: Option<Child>,
    /// A unique `pkill -f` pattern for children the direct kill misses.
    stragglers: Option<String>,
    mangohud_dir: Option<PathBuf>,
    ninja_dir: Option<PathBuf>,
    unsupported: Vec<String>,
    pinned_via_gamemode: bool,
}

/// Is the binary on `PATH`?
fn have(binary: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {binary} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn have_graphical_session() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok()
}

// ── MangoHud frametime log ───────────────────────────────────────────────────

/// Parse per-frame times out of a MangoHud CSV log.
///
/// MangoHud writes a provenance line, then a header row, then one row per
/// frame. The `frametime` column is microseconds. Rows whose frametime does not
/// parse (the trailing partial row of a killed process, blank lines) are
/// skipped rather than counted as zero-length frames.
pub fn parse_mangohud_frametimes_us(csv: &str) -> Vec<u64> {
    let mut column: Option<usize> = None;
    let mut out = Vec::new();
    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if column.is_none() {
            if let Some(idx) = fields
                .iter()
                .position(|f| f.eq_ignore_ascii_case("frametime"))
            {
                column = Some(idx);
            }
            continue;
        }
        let idx = column.expect("header seen");
        if let Some(raw) = fields.get(idx) {
            if let Ok(value) = raw.parse::<f64>() {
                if value > 0.0 {
                    out.push(value.round() as u64);
                }
            }
        }
    }
    out
}

/// Object files the build has produced so far.
pub fn count_object_files(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("o"))
        .count() as u64
}

/// Highest completed-edge count in `ninja`'s progress output (`[k/n]`).
pub fn parse_ninja_completed_edges(output: &str) -> u64 {
    let mut best = 0u64;
    for line in output.lines() {
        let line = line.trim_start();
        if !line.starts_with('[') {
            continue;
        }
        let Some(close) = line.find(']') else {
            continue;
        };
        let inner = &line[1..close];
        let Some((done, _total)) = inner.split_once('/') else {
            continue;
        };
        if let Ok(value) = done.trim().parse::<u64>() {
            best = best.max(value);
        }
    }
    best
}

// ── the deterministic throughput project ─────────────────────────────────────

/// Generator revision. The workload spec asks for "a pinned medium C++ project
/// (fixed revision)"; a generated project pins by generator version and unit
/// count instead of by git SHA, which is reproducible without network access.
pub const THROUGHPUT_PROJECT_REVISION: &str = "rushbench-cxx-2";
/// Translation units in the generated project.
///
/// Sized so the build **cannot finish inside a 60 s window**: one unit costs
/// about a second of a modern core, so a 24-thread machine clears roughly 1 300
/// of them per minute. At 96 units the first draft finished in ~5 s and the
/// remaining ~55 s of the "throughput" phase measured an idle machine
/// (`psi-cpu-avg10` came out at 0.06 %). A phase that goes quiescent halfway
/// through is not a throughput measurement.
///
/// A slower machine simply completes fewer units, which is exactly what
/// `joules-per-work-unit` normalizes away. A machine fast enough to drain the
/// whole project inside the window records
/// `throughput_build_completed_early` so the same defect cannot recur silently.
pub const THROUGHPUT_TRANSLATION_UNITS: usize = 1600;

/// Build parallelism for the throughput phase: twice the CPU count, so the run
/// queue is genuinely contended.
///
/// `ninja`'s own default (`nproc + 2`) leaves a 24-thread machine essentially
/// uncontended: a first draft measured `psi-cpu-avg10` at 0.06 % during a
/// saturated build, far under the 12.0 % the classifier needs to call
/// `throughput`, so the phase would have been classified `interactive` and
/// certified the wrong class. Oversubscribing is the same technique
/// `tools/bench-optid-host-v2.sh` already uses for its throughput scenario.
pub fn throughput_jobs() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    cpus.saturating_mul(2).max(2)
}

/// One translation unit's source. Template-instantiation heavy so each unit
/// costs roughly a second of a modern core, and pure so the compiler cannot
/// fold the work away.
pub fn throughput_translation_unit(index: usize) -> String {
    format!(
        "// {rev} unit {index}\n\
         #include <algorithm>\n\
         #include <map>\n\
         #include <string>\n\
         #include <vector>\n\
         \n\
         template <int N> struct Fold {{\n\
         \x20 static constexpr long long value = N + Fold<N - 1>::value;\n\
         }};\n\
         template <> struct Fold<0> {{ static constexpr long long value = 0; }};\n\
         \n\
         template <typename T, int Depth> struct Nest {{\n\
         \x20 using inner = Nest<std::map<T, std::vector<T>>, Depth - 1>;\n\
         \x20 static long long run(const T& seed) {{ return inner::run(std::map<T, std::vector<T>>{{{{seed, {{}}}}}}); }}\n\
         }};\n\
         template <typename T> struct Nest<T, 0> {{\n\
         \x20 static long long run(const T&) {{ return Fold<{index}>::value; }}\n\
         }};\n\
         \n\
         long long unit_{index}() {{\n\
         \x20 std::vector<std::string> keys;\n\
         \x20 for (int i = 0; i < 512; ++i) keys.push_back(std::to_string(i * {index} + 1));\n\
         \x20 std::sort(keys.begin(), keys.end());\n\
         \x20 return Nest<std::string, 6>::run(keys.front()) + static_cast<long long>(keys.size());\n\
         }}\n",
        rev = THROUGHPUT_PROJECT_REVISION,
        index = index
    )
}

/// `build.ninja` for a project of `units` translation units.
pub fn throughput_build_ninja(units: usize) -> String {
    let mut out = String::from("# generated by rushbench; revision ");
    out.push_str(THROUGHPUT_PROJECT_REVISION);
    out.push_str(
        "\ncxx = c++\ncflags = -O2 -std=c++17\n\nrule cc\n  command = $cxx $cflags -c $in -o $out\n  description = CC $out\n\n",
    );
    for i in 0..units {
        out.push_str(&format!("build tu{i:03}.o: cc tu{i:03}.cpp\n"));
    }
    out
}

/// Materialize the generated project, rewriting only what is missing or stale
/// so repeated cycles do not re-touch source timestamps.
fn ensure_throughput_project(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("throughput project dir: {e}"))?;
    for i in 0..THROUGHPUT_TRANSLATION_UNITS {
        let path = dir.join(format!("tu{i:03}.cpp"));
        let wanted = throughput_translation_unit(i);
        if fs::read_to_string(&path).ok().as_deref() != Some(wanted.as_str()) {
            fs::write(&path, &wanted).map_err(|e| format!("write {}: {e}", path.display()))?;
        }
    }
    let ninja_file = dir.join("build.ninja");
    let wanted = throughput_build_ninja(THROUGHPUT_TRANSLATION_UNITS);
    if fs::read_to_string(&ninja_file).ok().as_deref() != Some(wanted.as_str()) {
        fs::write(&ninja_file, &wanted).map_err(|e| format!("write build.ninja: {e}"))?;
    }
    Ok(())
}

// ── driver lifecycle ─────────────────────────────────────────────────────────

fn start_driver(
    driver: Driver,
    phase: &PhaseSpec,
    work_root: &Path,
    interactive_page: Option<&Path>,
) -> Result<RunningDriver, String> {
    let mut unsupported = Vec::new();
    match driver {
        Driver::Quiescent => Ok(RunningDriver {
            child: None,
            stragglers: None,
            mangohud_dir: None,
            ninja_dir: None,
            unsupported,
            pinned_via_gamemode: false,
        }),

        Driver::Interactive => {
            if !have("firefox") {
                unsupported.push("firefox not installed".to_string());
            }
            if !have_graphical_session() {
                unsupported.push("no graphical session for firefox".to_string());
            }
            let Some(page) = interactive_page else {
                unsupported.push("interactive load page missing from the repository".to_string());
                return Ok(RunningDriver {
                    child: None,
                    stragglers: None,
                    mangohud_dir: None,
                    ninja_dir: None,
                    unsupported,
                    pinned_via_gamemode: false,
                });
            };
            if !unsupported.is_empty() {
                return Ok(RunningDriver {
                    child: None,
                    stragglers: None,
                    mangohud_dir: None,
                    ninja_dir: None,
                    unsupported,
                    pinned_via_gamemode: false,
                });
            }
            let profile = work_root.join("firefox-profile");
            fs::create_dir_all(&profile).map_err(|e| format!("firefox profile dir: {e}"))?;
            let child = Command::new("firefox")
                .arg("--new-instance")
                .arg("--profile")
                .arg(&profile)
                .arg(format!("file://{}", page.display()))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("spawn firefox: {e}"))?;
            Ok(RunningDriver {
                child: Some(child),
                stragglers: Some(profile.display().to_string()),
                mangohud_dir: None,
                ninja_dir: None,
                unsupported,
                pinned_via_gamemode: false,
            })
        }

        Driver::Throughput => {
            if !have("ninja") {
                unsupported.push("ninja not installed".to_string());
            }
            if !have("c++") {
                unsupported.push("no c++ compiler".to_string());
            }
            if !unsupported.is_empty() {
                return Ok(RunningDriver {
                    child: None,
                    stragglers: None,
                    mangohud_dir: None,
                    ninja_dir: None,
                    unsupported,
                    pinned_via_gamemode: false,
                });
            }
            let dir = work_root.join("throughput-project");
            ensure_throughput_project(&dir)?;
            // Every cycle must compile the same amount of work, so discard the
            // previous cycle's objects before starting.
            let _ = Command::new("ninja")
                .arg("-C")
                .arg(&dir)
                .arg("-t")
                .arg("clean")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let child = Command::new("ninja")
                .arg("-C")
                .arg(&dir)
                .arg("-j")
                .arg(throughput_jobs().to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("spawn ninja: {e}"))?;
            Ok(RunningDriver {
                child: Some(child),
                stragglers: Some(dir.display().to_string()),
                mangohud_dir: None,
                ninja_dir: Some(dir),
                unsupported,
                pinned_via_gamemode: false,
            })
        }

        Driver::LatencyCritical => {
            if !have("glmark2") {
                unsupported.push("glmark2 not installed".to_string());
            }
            if !have_graphical_session() {
                unsupported.push("no graphical session for glmark2".to_string());
            }
            let instrumented = have("mangohud");
            if !instrumented {
                unsupported.push(
                    "mangohud not installed: frametime percentiles are unavailable".to_string(),
                );
            }
            if !unsupported.is_empty() && !instrumented {
                // glmark2 alone reports per-scene average FPS, not a frametime
                // distribution, so without MangoHud there is nothing to
                // percentile even if glmark2 itself runs.
            }
            if unsupported.iter().any(|u| u.starts_with("glmark2"))
                || unsupported.iter().any(|u| u.starts_with("no graphical"))
            {
                return Ok(RunningDriver {
                    child: None,
                    stragglers: None,
                    mangohud_dir: None,
                    ninja_dir: None,
                    unsupported,
                    pinned_via_gamemode: false,
                });
            }
            // Phase 4's class cannot be inferred on battery — `policy.rs`'s
            // `LatencyCritical` branch requires `on_ac == Some(true)`, which
            // Criterion 3 (battery) makes unreachable by construction. Wrap
            // the driver with `gamemoderun` so it registers with optid's
            // `com.feralinteractive.GameMode` shim — the same path Steam and
            // Lutris use — asserting the class instead of relying on the
            // classifier to infer it. See
            // docs/inbox/2026-08-22-phase-d-latency-critical-blocked.md
            // (decision A).
            let pinned = have("gamemoderun");
            if !pinned {
                unsupported.push(
                    "gamemoderun not installed: phase 4's class is asserted via the GameMode \
                     shim in production; without it this phase falls back to observing the \
                     classifier, which cannot reach latency-critical on battery"
                        .to_string(),
                );
            }
            let log_dir = work_root.join("mangohud");
            let _ = fs::remove_dir_all(&log_dir);
            fs::create_dir_all(&log_dir).map_err(|e| format!("mangohud log dir: {e}"))?;
            let seconds = phase.duration.as_secs();
            let (program, chain): (&str, &[&str]) = match (pinned, instrumented) {
                (true, true) => ("gamemoderun", &["mangohud", "glmark2"]),
                (true, false) => ("gamemoderun", &["glmark2"]),
                (false, true) => ("mangohud", &["glmark2"]),
                (false, false) => ("glmark2", &[]),
            };
            let mut command = Command::new(program);
            command.args(chain);
            if instrumented {
                command.env(
                    "MANGOHUD_CONFIG",
                    format!(
                        "autostart_log=1,log_duration={seconds},output_folder={},no_display=1",
                        log_dir.display()
                    ),
                );
            }
            command
                .arg("--run-forever")
                .arg("--fullscreen")
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = command.spawn().map_err(|e| format!("spawn glmark2: {e}"))?;
            Ok(RunningDriver {
                child: Some(child),
                stragglers: Some("glmark2".to_string()),
                mangohud_dir: if instrumented { Some(log_dir) } else { None },
                ninja_dir: None,
                unsupported,
                pinned_via_gamemode: pinned,
            })
        }
    }
}

fn stop_driver(driver: Driver, mut running: RunningDriver) -> DriverOutcome {
    // `start_driver` already reported its own unsupported reasons; this only
    // adds what the harvest itself discovers.
    let mut outcome = DriverOutcome {
        pinned_via_gamemode: running.pinned_via_gamemode,
        ..DriverOutcome::default()
    };

    let mut ninja_stdout = String::new();
    if let Some(mut child) = running.child.take() {
        let still_running = matches!(child.try_wait(), Ok(None));
        let _ = child.kill();
        if let Ok(out) = child.wait_with_output() {
            ninja_stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        }
        if !still_running && driver != Driver::Throughput {
            outcome
                .unsupported
                .push("driver exited before the phase window closed".to_string());
        }
    }
    if let Some(pattern) = running.stragglers.take() {
        let _ = Command::new("pkill")
            .arg("-f")
            .arg(&pattern)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    if let Some(dir) = running.ninja_dir.take() {
        // Killing `ninja` mid-build discards whatever progress output was still
        // buffered in its pipe, so the object files it produced are the
        // authoritative count. Parsed progress still wins when the build ran to
        // completion and flushed.
        let objects = count_object_files(&dir);
        let edges = parse_ninja_completed_edges(&ninja_stdout);
        let units = objects.max(edges);
        if units == 0 {
            outcome
                .unsupported
                .push(format!("ninja completed no edges in {}", dir.display()));
        }
        if units >= THROUGHPUT_TRANSLATION_UNITS as u64 {
            // The build drained before the window closed, so the tail of the
            // phase measured an idle machine, not throughput.
            outcome.unsupported.push(
                "throughput_build_completed_early: the phase went quiescent before \
                       the window closed; raise THROUGHPUT_TRANSLATION_UNITS"
                    .to_string(),
            );
        }
        outcome.work_units = Some(units);
    }

    if let Some(dir) = running.mangohud_dir.take() {
        let mut frames = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("csv") {
                    if let Ok(text) = fs::read_to_string(&path) {
                        frames.extend(parse_mangohud_frametimes_us(&text));
                    }
                }
            }
        }
        if frames.is_empty() {
            outcome
                .unsupported
                .push("mangohud wrote no frametime rows".to_string());
        }
        outcome.frametimes_us = frames;
    }

    outcome
}

// ── the sequencer ────────────────────────────────────────────────────────────

/// One phase's measured samples, keyed by metric.
type PhaseSamples = BTreeMap<String, Vec<u64>>;

struct PhaseAccumulator {
    samples: PhaseSamples,
    anomalies: Vec<String>,
    classes_observed: Vec<String>,
    energy: Vec<EnergyInfo>,
}

impl PhaseAccumulator {
    fn new() -> Self {
        Self {
            samples: BTreeMap::new(),
            anomalies: Vec::new(),
            classes_observed: Vec::new(),
            energy: Vec::new(),
        }
    }

    fn note(&mut self, anomaly: String) {
        if !self.anomalies.contains(&anomaly) {
            self.anomalies.push(anomaly);
        }
    }
}

/// The class `optid` currently reports, or `None` when no daemon answers.
fn observe_class() -> Option<String> {
    let json = get_optctl_status_json().ok()?;
    let status = parse_optctl_status(&json).ok()?;
    Some(status.workload_class)
}

/// The anomaly (if any) to record for one phase's observed class.
///
/// Three of the four load-producing phases leave the class to the classifier;
/// a disagreement there is `class_mismatch` — evidence the classifier didn't
/// infer what was expected, never a hard error. Phase 4 is asserted via
/// `gamemoderun`'s `RegisterGame` call instead (see the module docs), so a
/// disagreement there means the *assertion* failed — a different, more
/// serious anomaly — and agreement is itself worth recording, since it was
/// not inferred.
fn class_observation_anomaly(
    observed: &str,
    expected_class: &str,
    pinned_via_gamemode: bool,
) -> Option<String> {
    match (observed == expected_class, pinned_via_gamemode) {
        (true, true) => Some("class_pinned_via_gamemode".to_string()),
        (true, false) => None,
        (false, true) => Some(format!("class_pin_ineffective:{observed}")),
        (false, false) => Some(format!("class_mismatch:{observed}")),
    }
}

fn optid_version() -> String {
    let candidates = [
        std::env::var("RUSHBENCH_OPTID_BIN").unwrap_or_default(),
        "optid".to_string(),
        "./target/release/optid".to_string(),
    ];
    for candidate in candidates.iter().filter(|c| !c.is_empty()) {
        if let Ok(out) = Command::new(candidate).arg("-V").output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // The 2026-06-10 capture recorded usage text here; refuse it.
                if !text.is_empty() && !text.to_lowercase().starts_with("usage") {
                    return text;
                }
            }
        }
    }
    "unavailable".to_string()
}

fn read_first_line(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
}

fn battery_percent() -> Option<u64> {
    let root = crate::utils::get_sysfs_root().join("sys/class/power_supply");
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if fs::read_to_string(path.join("type"))
            .map(|t| t.trim().eq_ignore_ascii_case("Battery"))
            .unwrap_or(false)
        {
            return fs::read_to_string(path.join("capacity"))
                .ok()
                .and_then(|c| c.trim().parse().ok());
        }
    }
    None
}

/// Write `meta.txt` in the Dragnet schema the evidence gate checks.
fn write_meta(dir: &Path, lever: &str, started_at: &str) -> Result<(), String> {
    let host = read_first_line("/proc/sys/kernel/hostname").unwrap_or_else(|| "unknown".into());
    let ncpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let driver = read_first_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver")
        .unwrap_or_else(|| "unknown".into());
    let governor = read_first_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .unwrap_or_else(|| "unknown".into());
    let platform_profile = if Path::new("/sys/firmware/acpi/platform_profile").exists() {
        "1"
    } else {
        "0"
    };
    let meta = format!(
        "date={started_at}\n\
         host={host}\n\
         kernel={kernel}\n\
         cpu={cpu}\n\
         ncpu={ncpu}\n\
         cpufreq_driver={driver}\n\
         governor={governor}\n\
         platform_profile_available={platform_profile}\n\
         rapl_domain=/sys/class/powercap/intel-rapl:0\n\
         optid_version={optid}\n\
         git_commit={git}\n\
         lever={lever}\n",
        kernel = get_kernel_version(),
        cpu = get_cpu_model(),
        optid = optid_version(),
        git = get_git_sha().unwrap_or_else(|_| "unknown".to_string()),
    );
    fs::write(dir.join("meta.txt"), meta).map_err(|e| format!("write meta.txt: {e}"))
}

/// Run `cycles` cycles of `preset` and write the evidence artifacts.
pub fn run_preset(
    preset: &str,
    cycles: usize,
    tag: &str,
    out_dir: &Path,
    ac_ok: bool,
) -> Result<(), String> {
    let phases = preset_phases(preset)?;
    let started_at = get_utc_timestamp();
    let lever = if tag.starts_with("optid") {
        "optid"
    } else {
        "baseline"
    };

    let on_ac = read_on_ac();
    let power_source = if on_ac == Some(true) { "ac" } else { "battery" };
    if power_source == "ac" && !ac_ok {
        return Err(
            "Refusing to run on AC power: Criterion 3 is an on-battery measurement. \
             Unplug, or pass --ac-ok for a Criterion 2-only run."
                .to_string(),
        );
    }

    let energy_source = EnergySource::detect().map_err(|e| format!("no_energy_counter: {e}"))?;
    // A battery charge counter measures nothing while the charger holds the pack
    // full, so on AC it would report a real-looking 0 W instead of refusing.
    let energy_is_measurable =
        !(power_source == "ac" && matches!(energy_source, EnergySource::Battery(_)));

    let work_root = std::env::var("RUSHBENCH_WORK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("rushbench-mixed-load-001"));
    fs::create_dir_all(&work_root).map_err(|e| format!("work dir: {e}"))?;
    let interactive_page = find_repo_file("benchmarks/fixtures/interactive-load.html");

    fs::create_dir_all(out_dir).map_err(|e| format!("output dir: {e}"))?;

    let mut transcript: Vec<String> = Vec::new();
    let say = |line: String, log: &mut Vec<String>| {
        println!("{line}");
        log.push(line);
    };

    say(
        format!("rushbench preset={preset} tag={tag} lever={lever} cycles={cycles}"),
        &mut transcript,
    );
    say(
        format!("started_at={started_at} power_source={power_source} on_ac={on_ac:?}"),
        &mut transcript,
    );
    say(
        format!(
            "energy_counter={} optid_version={}",
            match &energy_source {
                EnergySource::Battery(p) => p.display().to_string(),
                EnergySource::Rapl(p) => p.display().to_string(),
            },
            optid_version()
        ),
        &mut transcript,
    );
    say(
        format!(
            "throughput_project={THROUGHPUT_PROJECT_REVISION} units={THROUGHPUT_TRANSLATION_UNITS} \
             jobs={}",
            throughput_jobs()
        ),
        &mut transcript,
    );
    say(
        "sample units: psi-* and frametime-*/discharge-w/joules-per-work-unit samples are \
         milli-units (x1000); results.csv medians are in the metric's declared unit"
            .to_string(),
        &mut transcript,
    );
    let scale = phase_scale();
    if scale != 1 {
        say(
            format!(
                "WARNING: RUSHBENCH_PHASE_SCALE={scale} shortened every phase window; \
                 this run is harness validation, NOT milestone evidence"
            ),
            &mut transcript,
        );
    }
    if cycles < REQUIRED_CYCLES {
        say(
            format!(
                "WARNING: cycles={cycles} < {REQUIRED_CYCLES}; every record will carry \
                 insufficient_n and is NOT milestone evidence"
            ),
            &mut transcript,
        );
    }

    // A battery counter pinned at full charge reports nothing for the first
    // minutes of a run, so the first cycle measured no energy at all and the
    // step that followed landed on a later window as an impossible 188 W. Wait
    // for the counter to actually move before starting cycle 1.
    if on_ac != Some(true) {
        let warmup_limit = Duration::from_secs(
            std::env::var("RUSHBENCH_COUNTER_WARMUP_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        );
        match energy_source.sample() {
            Ok(first) => {
                let started = Instant::now();
                let mut moved = false;
                while started.elapsed() < warmup_limit {
                    std::thread::sleep(Duration::from_secs(5));
                    match energy_source.sample() {
                        Ok(now) if now.joules != first.joules => {
                            moved = true;
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                if moved {
                    say(
                        format!(
                            "energy counter moved after {:.0} s; starting cycles",
                            started.elapsed().as_secs_f64()
                        ),
                        &mut transcript,
                    );
                } else {
                    say(
                        format!(
                            "WARNING: energy counter did not move in {:.0} s; energy windows \
                             may be rejected",
                            warmup_limit.as_secs_f64()
                        ),
                        &mut transcript,
                    );
                }
            }
            Err(error) => say(
                format!("WARNING: could not sample the energy counter: {error}"),
                &mut transcript,
            ),
        }
    }

    let mut accumulators: Vec<PhaseAccumulator> =
        phases.iter().map(|_| PhaseAccumulator::new()).collect();

    for cycle in 1..=cycles {
        for (index, phase) in phases.iter().enumerate() {
            let acc = &mut accumulators[index];
            say(
                format!(
                    "cycle {cycle}/{cycles} phase {} ({}s, expect class {})",
                    phase.name,
                    phase.duration.as_secs(),
                    phase.expected_class
                ),
                &mut transcript,
            );

            let running =
                start_driver(phase.driver, phase, &work_root, interactive_page.as_deref())?;
            for reason in &running.unsupported {
                acc.note(format!("unsupported_here: {reason}"));
                say(format!("  unsupported_here: {reason}"), &mut transcript);
            }

            let energy_start: Option<EnergySample> = energy_source.sample().ok();
            let window_start = Instant::now();

            // Probes that must be exercised while the load is live.
            let mut mid_window: BTreeMap<String, ProbeResult> = BTreeMap::new();
            let half = phase.duration / 2;
            std::thread::sleep(half);
            for metric in phase.metrics {
                if *metric == "foreground-launch-ms" {
                    mid_window.insert((*metric).to_string(), run_probe_for_metric(metric));
                }
            }
            let remaining = phase.duration.saturating_sub(window_start.elapsed());
            std::thread::sleep(remaining);

            let energy_end: Option<EnergySample> = energy_source.sample().ok();
            let elapsed = window_start.elapsed();
            let outcome = stop_driver(phase.driver, running);
            for reason in &outcome.unsupported {
                acc.note(format!("unsupported_here: {reason}"));
                say(format!("  unsupported_here: {reason}"), &mut transcript);
            }

            let energy = if !energy_is_measurable {
                acc.note(
                    "unsupported_here: energy: battery counter cannot measure a window on AC"
                        .to_string(),
                );
                None
            } else {
                match (energy_start, energy_end) {
                    (Some(a), Some(b)) => match calculate_window(&energy_source, &a, &b) {
                        Ok(info) => {
                            say(
                                format!(
                                    "  energy {:.2} J over {:.1} s ({:.2} W) via {}",
                                    info.window_joules,
                                    elapsed.as_secs_f64(),
                                    info.avg_watts,
                                    info.counter
                                ),
                                &mut transcript,
                            );
                            acc.energy.push(info.clone());
                            Some(info)
                        }
                        Err(reason) => {
                            acc.note(format!("energy_window_rejected: {reason}"));
                            say(
                                format!("  energy_window_rejected: {reason}"),
                                &mut transcript,
                            );
                            None
                        }
                    },
                    _ => {
                        acc.note("energy_sample_failed".to_string());
                        None
                    }
                }
            };

            match observe_class() {
                Some(observed) => {
                    acc.classes_observed.push(observed.clone());
                    if let Some(anomaly) = class_observation_anomaly(
                        &observed,
                        phase.expected_class,
                        outcome.pinned_via_gamemode,
                    ) {
                        acc.note(anomaly);
                    }
                    say(format!("  class_observed={observed}"), &mut transcript);
                }
                None => {
                    acc.note("optid_absent".to_string());
                    say(
                        "  class_observed=unmeasured (no optid)".to_string(),
                        &mut transcript,
                    );
                }
            }

            for metric in phase.metrics {
                let metric = *metric;
                let sample: Option<u64> = match metric {
                    "frametime-p95-ms" | "frametime-p99-ms" => {
                        if outcome.frametimes_us.is_empty() {
                            None
                        } else {
                            let mut sorted = outcome.frametimes_us.clone();
                            sorted.sort_unstable();
                            let pct = if metric.contains("p95") { 0.95 } else { 0.99 };
                            Some(percentile(&sorted, pct).round() as u64)
                        }
                    }
                    "discharge-w" => energy
                        .as_ref()
                        .map(|e| (e.avg_watts * 1000.0).round() as u64),
                    "joules-per-work-unit" => match (energy.as_ref(), outcome.work_units) {
                        (Some(e), Some(units)) if units > 0 => {
                            Some(((e.window_joules / units as f64) * 1000.0).round() as u64)
                        }
                        _ => None,
                    },
                    "foreground-launch-ms" => match mid_window.get(metric) {
                        Some(ProbeResult::Success(value)) => Some(*value),
                        Some(ProbeResult::UnsupportedHere(reason)) => {
                            acc.note(format!("unsupported_here: {metric}: {reason}"));
                            None
                        }
                        Some(ProbeResult::Failed(reason)) => {
                            acc.note(format!("probe_failed: {metric}: {reason}"));
                            None
                        }
                        None => None,
                    },
                    _ => match run_probe_for_metric(metric) {
                        ProbeResult::Success(value) => Some(value),
                        ProbeResult::UnsupportedHere(reason) => {
                            acc.note(format!("unsupported_here: {metric}: {reason}"));
                            None
                        }
                        ProbeResult::Failed(reason) => {
                            acc.note(format!("probe_failed: {metric}: {reason}"));
                            None
                        }
                    },
                };

                match sample {
                    Some(value) => {
                        let (unit, divisor) = metric_unit(metric);
                        say(
                            format!("  {metric}={:.3} {unit}", value as f64 / divisor),
                            &mut transcript,
                        );
                        acc.samples
                            .entry(metric.to_string())
                            .or_default()
                            .push(value);
                    }
                    None => {
                        // Energy-derived metrics fail for a reason recorded
                        // against the window, not against the metric name.
                        let energy_derived =
                            matches!(metric, "discharge-w" | "joules-per-work-unit");
                        let reason = acc
                            .anomalies
                            .iter()
                            .rev()
                            .find(|a| {
                                a.contains(metric) || (energy_derived && a.contains("energy"))
                            })
                            .cloned()
                            .unwrap_or_else(|| "no sample produced".to_string());
                        say(
                            format!("  {metric}=unavailable ({reason})"),
                            &mut transcript,
                        );
                    }
                }
            }
        }
    }

    // ── artifacts ───────────────────────────────────────────────────────────
    let contracts = find_repo_file("config/optid/contracts.toml")
        .map(|p| parse_contracts_toml(&p))
        .unwrap_or_default();

    let mut csv =
        String::from("phase,lever,scenario,metric,median,iters,batt_pct,ambient_cpu_pct\n");
    let batt = battery_percent()
        .map(|b| b.to_string())
        .unwrap_or_else(|| "na".to_string());
    let ambient = match run_probe_for_metric("psi-cpu-avg10") {
        ProbeResult::Success(v) => format!("{:.2}", v as f64 / 1000.0),
        _ => "na".to_string(),
    };

    for (index, phase) in phases.iter().enumerate() {
        let acc = &accumulators[index];
        let class_observed = acc
            .classes_observed
            .last()
            .cloned()
            .unwrap_or_else(|| "unmeasured".to_string());
        let phase_energy = acc.energy.last().cloned();

        for metric in phase.metrics {
            let metric = *metric;
            let samples = acc.samples.get(metric).cloned().unwrap_or_default();
            let n = samples.len();
            let mut anomalies = acc.anomalies.clone();
            if n < REQUIRED_CYCLES {
                anomalies.push("insufficient_n".to_string());
            }
            if n == 0 {
                anomalies.push("no_samples".to_string());
            }
            if scale != 1 {
                anomalies.push(format!("phase_scale_shortened:{scale}"));
            }

            let record = build_record(
                phase.expected_class,
                &class_observed,
                phase.name,
                metric,
                n,
                if samples.is_empty() {
                    None
                } else {
                    Some(samples.clone())
                },
                phase_energy.clone(),
                &started_at,
                0,
                anomalies,
                power_source,
                contracts
                    .get(phase.expected_class)
                    .map(|c| c.cpu_wakeup_latency)
                    .unwrap_or(-1),
                contracts
                    .get(phase.expected_class)
                    .map(|c| c.device_resume_latency)
                    .unwrap_or(-1),
            );

            let json = serde_json::to_string_pretty(&record)
                .map_err(|e| format!("serialize record: {e}"))?;
            let file = out_dir.join(format!("{}__{metric}.json", phase.name));
            fs::write(&file, json).map_err(|e| format!("write {}: {e}", file.display()))?;

            let (_, divisor) = metric_unit(metric);
            let median = match record.median {
                Some(m) => format!("{:.3}", m / divisor),
                None => "na".to_string(),
            };
            csv.push_str(&format!(
                "{},{lever},{preset},{metric},{median},{n},{batt},{ambient}\n",
                phase.name
            ));
        }
    }

    fs::write(out_dir.join("results.csv"), &csv).map_err(|e| format!("write results.csv: {e}"))?;
    write_meta(out_dir, lever, &started_at)?;
    transcript.push(format!("finished_at={}", get_utc_timestamp()));
    fs::write(out_dir.join("transcript.log"), transcript.join("\n") + "\n")
        .map_err(|e| format!("write transcript.log: {e}"))?;

    println!("Wrote {} arm artifacts to {}", lever, out_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_rejects_unknown_names() {
        assert!(preset_phases("mixed-load-002").is_err());
    }

    #[test]
    fn preset_walks_the_five_specified_phases_in_order() {
        let phases = preset_phases(MIXED_LOAD_001).expect("preset exists");
        let names: Vec<&str> = phases.iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            vec![
                "idle-warm",
                "interactive",
                "throughput",
                "latency-critical",
                "idle-cooldown"
            ]
        );
        let seconds: Vec<u64> = phases.iter().map(|p| p.duration.as_secs()).collect();
        assert_eq!(seconds, vec![60, 60, 60, 60, 30]);
        // One cycle is the spec's 4 min 30 s.
        assert_eq!(seconds.iter().sum::<u64>(), 270);
    }

    #[test]
    fn scale_of_one_keeps_the_specified_duration() {
        assert_eq!(scaled_duration(60, 1), Duration::from_secs(60));
        assert_eq!(scaled_duration(30, 1), Duration::from_secs(30));
    }

    #[test]
    fn scaling_shortens_windows_but_never_below_one_second() {
        assert_eq!(scaled_duration(60, 20), Duration::from_secs(3));
        assert_eq!(scaled_duration(30, 60), Duration::from_secs(1));
        // A zero scale must not divide by zero or lengthen the window.
        assert_eq!(scaled_duration(60, 0), Duration::from_secs(60));
    }

    #[test]
    fn preset_covers_every_load_producing_class() {
        let phases = preset_phases(MIXED_LOAD_001).expect("preset exists");
        for class in ["idle", "interactive", "throughput", "latency-critical"] {
            assert!(
                phases.iter().any(|p| p.expected_class == class),
                "no phase drives {class}"
            );
        }
    }

    #[test]
    fn unpinned_matching_class_has_no_anomaly() {
        assert_eq!(
            class_observation_anomaly("interactive", "interactive", false),
            None
        );
    }

    #[test]
    fn unpinned_mismatch_is_class_mismatch() {
        assert_eq!(
            class_observation_anomaly("light", "latency-critical", false),
            Some("class_mismatch:light".to_string())
        );
    }

    #[test]
    fn pinned_matching_class_is_recorded_as_pinned() {
        assert_eq!(
            class_observation_anomaly("latency-critical", "latency-critical", true),
            Some("class_pinned_via_gamemode".to_string())
        );
    }

    #[test]
    fn pinned_mismatch_is_pin_ineffective_not_class_mismatch() {
        assert_eq!(
            class_observation_anomaly("light", "latency-critical", true),
            Some("class_pin_ineffective:light".to_string())
        );
    }

    #[test]
    fn mangohud_log_yields_per_frame_microseconds() {
        let csv = "mangohud,v0.8.3,glmark2\n\
                   fps,frametime,cpu_load,gpu_load\n\
                   60.1,16640,12,44\n\
                   59.8,16720,13,45\n\
                   58.2,17180,14,46\n";
        assert_eq!(parse_mangohud_frametimes_us(csv), vec![16640, 16720, 17180]);
    }

    #[test]
    fn mangohud_log_skips_blank_and_partial_rows() {
        let csv = "fps,frametime,cpu_load\n\
                   60.0,16600,10\n\
                   \n\
                   59.5,,11\n\
                   59.0,0,11\n\
                   58.0,17000,12\n";
        assert_eq!(parse_mangohud_frametimes_us(csv), vec![16600, 17000]);
    }

    #[test]
    fn mangohud_log_without_a_frametime_column_yields_nothing() {
        assert!(parse_mangohud_frametimes_us("fps,cpu_load\n60,10\n").is_empty());
    }

    #[test]
    fn ninja_progress_reports_the_highest_completed_edge_count() {
        let out = "[1/96] CC tu000.o\n[2/96] CC tu001.o\n[17/96] CC tu016.o\n";
        assert_eq!(parse_ninja_completed_edges(out), 17);
    }

    #[test]
    fn ninja_progress_of_a_build_that_never_started_is_zero() {
        assert_eq!(parse_ninja_completed_edges("ninja: no work to do.\n"), 0);
    }

    #[test]
    fn ninja_progress_ignores_non_progress_lines() {
        let out = "ninja: Entering directory `/tmp/x'\n[3/96] CC tu002.o\nFAILED: tu003.o\n";
        assert_eq!(parse_ninja_completed_edges(out), 3);
    }

    #[test]
    fn throughput_oversubscribes_the_cpu_count() {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        assert_eq!(throughput_jobs(), cpus * 2);
        assert!(throughput_jobs() >= 2);
    }

    #[test]
    fn generated_project_is_byte_identical_across_calls() {
        assert_eq!(
            throughput_translation_unit(7),
            throughput_translation_unit(7)
        );
        assert_ne!(
            throughput_translation_unit(7),
            throughput_translation_unit(8)
        );
        let ninja = throughput_build_ninja(3);
        assert!(ninja.contains("build tu000.o: cc tu000.cpp"));
        assert!(ninja.contains("build tu002.o: cc tu002.cpp"));
        assert!(!ninja.contains("tu003"));
    }

    #[test]
    fn object_files_are_counted_and_other_files_ignored() {
        let dir = std::env::temp_dir().join("rushbench-count-objects-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        for name in [
            "tu000.o",
            "tu001.o",
            "tu000.cpp",
            "build.ninja",
            ".ninja_log",
        ] {
            fs::write(dir.join(name), "x").expect("fixture file");
        }
        assert_eq!(count_object_files(&dir), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn object_count_of_a_missing_directory_is_zero() {
        assert_eq!(
            count_object_files(Path::new("/nonexistent/rushbench/throughput")),
            0
        );
    }

    #[test]
    fn metric_units_scale_milli_units_back_to_declared_units() {
        assert_eq!(metric_unit("frametime-p99-ms"), ("ms", 1000.0));
        assert_eq!(metric_unit("discharge-w"), ("W", 1000.0));
        assert_eq!(metric_unit("foreground-launch-ms"), ("ms", 1.0));
        assert_eq!(metric_unit("psi-cpu-avg10"), ("percent", 1000.0));
    }
}
