//! Deterministic simulated evidence for one question:
//!
//! > When optid is fully enabled, does it theoretically improve the modelled
//! > system compared with optid off, while remaining safe under faults and
//! > recovery?
//!
//! The harness drives the unmodified production control loop — `crate::run` —
//! against a simulated machine materialised inside a verified simulation root.
//! Sensing, classification, policy, the domain-mode gate, the hardware
//! allowlist, the contract gate, the capability gate, the reconciler's
//! transactions, the circuit breaker, actuation, journalling and shutdown
//! restoration are all the real code. Only the machine underneath is modelled.
//!
//! Nothing here measures physical hardware. Every latency, throughput, energy
//! and temperature number is computed by `model.rs` from the simulated machine
//! state, and carries no claim about any real device.

pub(crate) mod analysis;
pub(crate) mod machine;
pub(crate) mod model;
pub(crate) mod report;
pub(crate) mod scenarios;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::args::Args;
use crate::kernel_io::with_real_kernel_override;

use machine::{SimFault, SimKernel, SimMachine, SimPmqosSink, StepSample, WriteRecord};
use model::Assumptions;
use scenarios::{Arm, ArmKind, PolicyFlavour, Scenario};

pub(crate) const BUNDLE_SCHEMA_VERSION: u32 = 1;
const ROOT_MARKER: &str = ".optid-evidence-root-v1";
const ROOT_MARKER_CONTENT: &str = "optid-simulation-evidence-root-v1\n";
const DEFAULT_REPEATS: u32 = 3;

/// Command-line request for the evidence harness.
pub(crate) struct EvidenceOptions {
    pub(crate) root: PathBuf,
    pub(crate) repeats: u32,
}

/// Recognise `--evidence-root` before the daemon argument parser runs. Returns
/// `None` when this invocation is an ordinary daemon start.
pub(crate) fn extract_evidence_options(args: &[String]) -> Result<Option<EvidenceOptions>, String> {
    let mut root: Option<PathBuf> = None;
    let mut repeats = DEFAULT_REPEATS;
    let mut iter = args.iter();
    let mut saw = false;
    while let Some(arg) = iter.next() {
        if arg == "--evidence-root" {
            saw = true;
            root =
                Some(PathBuf::from(iter.next().ok_or_else(|| {
                    "--evidence-root requires a path".to_string()
                })?));
        } else if let Some(value) = arg.strip_prefix("--evidence-root=") {
            saw = true;
            root = Some(PathBuf::from(value));
        } else if arg == "--evidence-repeats" {
            repeats = iter
                .next()
                .ok_or_else(|| "--evidence-repeats requires a value".to_string())?
                .parse::<u32>()
                .map_err(|_| "--evidence-repeats must be an integer".to_string())?;
        } else if let Some(value) = arg.strip_prefix("--evidence-repeats=") {
            repeats = value
                .parse::<u32>()
                .map_err(|_| "--evidence-repeats must be an integer".to_string())?;
        }
    }
    if !saw {
        return Ok(None);
    }
    let root = root.ok_or_else(|| "--evidence-root requires a path".to_string())?;
    if repeats < 2 {
        return Err("--evidence-repeats must be at least 2 to prove determinism".to_string());
    }
    Ok(Some(EvidenceOptions { root, repeats }))
}

/// Validate the simulation root. The root must be an absolute, non-symlink
/// directory that is not `/`, carries the marker file with the exact expected
/// content, and contains no `..` component.
fn validate_root(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err(format!(
            "simulation root must be an absolute path: {}",
            root.display()
        ));
    }
    if root
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("simulation root must not contain a parent-directory component".to_string());
    }
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "simulation root must not be a symlink: {}",
            root.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "simulation root must be a directory: {}",
            root.display()
        ));
    }
    let canonical = fs::canonicalize(root)
        .map_err(|error| format!("cannot canonicalize {}: {error}", root.display()))?;
    if canonical == Path::new("/") {
        return Err("simulation root must not be the filesystem root".to_string());
    }
    for forbidden in [
        "/sys", "/proc", "/run", "/dev", "/etc", "/var", "/usr", "/boot",
    ] {
        if canonical == Path::new(forbidden) || canonical.starts_with(forbidden) {
            return Err(format!(
                "simulation root must not be inside {forbidden}: {}",
                canonical.display()
            ));
        }
    }
    let marker = canonical.join(ROOT_MARKER);
    let marker_meta = fs::symlink_metadata(&marker).map_err(|_| {
        format!(
            "simulation root is unmarked; create {} containing {:?}",
            marker.display(),
            ROOT_MARKER_CONTENT
        )
    })?;
    if marker_meta.file_type().is_symlink() || !marker_meta.is_file() {
        return Err("simulation root marker must be a regular file".to_string());
    }
    let content = fs::read_to_string(&marker)
        .map_err(|error| format!("cannot read {}: {error}", marker.display()))?;
    if content != ROOT_MARKER_CONTENT {
        return Err("simulation root marker has unexpected content".to_string());
    }
    Ok(canonical)
}

/// Process-level containment guards. These are belt-and-braces on top of the
/// kernel adapter: even a code path that bypassed the seam entirely must not be
/// able to reach a system service.
fn install_process_guards(root: &Path) -> Result<Vec<String>, String> {
    let mut guards = Vec::new();
    let empty_bin = root.join("guard/empty-bin");
    fs::create_dir_all(&empty_bin)
        .map_err(|error| format!("cannot create {}: {error}", empty_bin.display()))?;
    std::env::set_var("PATH", &empty_bin);
    guards.push(format!(
        "PATH restricted to an empty directory inside the simulation root ({}), so no `systemctl` \
         or any other host binary can be executed",
        empty_bin.display()
    ));

    let bus = root.join("guard/no-system-bus");
    std::env::set_var(
        "DBUS_SYSTEM_BUS_ADDRESS",
        format!("unix:path={}", bus.display()),
    );
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={}", bus.display()),
    );
    guards.push(format!(
        "DBUS_SYSTEM_BUS_ADDRESS and DBUS_SESSION_BUS_ADDRESS point at a non-existent socket \
         inside the simulation root ({}), so no system service can be reached or claimed",
        bus.display()
    ));

    // The harness ends each daemon run by raising SIGTERM. Registering a flag
    // here first guarantees the process disposition is never "terminate", even
    // if the daemon's own registration were to fail.
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, std::sync::Arc::clone(&flag))
        .map_err(|error| format!("cannot install the harness SIGTERM guard: {error}"))?;
    guards.push(
        "a process-wide SIGTERM flag handler is installed before any run, so the harness's own \
         clean-shutdown signal can never terminate the process"
            .to_string(),
    );

    // The conflict detector shells out to `systemctl`. Pin it to a deterministic
    // answer so no subprocess is attempted at all.
    crate::shim::conflict::set_conflict_checker_override(Some(no_conflicting_daemon));
    guards.push(
        "the competing-daemon detector is pinned to a deterministic \"no conflict\" answer, so it \
         never spawns a process"
            .to_string(),
    );
    guards.push(
        "the only process the harness starts is the sibling `optid-recover` executable, by \
         absolute path, with `--machine-root` pointing inside the simulation root"
            .to_string(),
    );
    Ok(guards)
}

fn no_conflicting_daemon(_service: &str) -> bool {
    false
}

/// One arm run against one scenario, once.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Trial {
    pub(crate) arm: String,
    pub(crate) scenario: String,
    pub(crate) repeat: u32,
    pub(crate) daemon_outcome: String,
    pub(crate) recovery_outcome: Option<String>,
    /// Every S3D one-shot recovery pass the supervisor ran before starting
    /// optid, in order.
    pub(crate) s3d_recovery: Vec<S3dRecovery>,
    pub(crate) cycles_completed: u32,
    pub(crate) host_write_attempts: u64,
    pub(crate) containment_violations: Vec<String>,
    pub(crate) writes: Vec<WriteRecord>,
    pub(crate) samples: Vec<StepSample>,
    pub(crate) receipts: Vec<analysis::Receipt>,
    pub(crate) envelope: analysis::EnvelopeSummary,
    pub(crate) aggregate: BTreeMap<String, f64>,
    pub(crate) sensitivity: BTreeMap<String, BTreeMap<String, f64>>,
    pub(crate) rejections: Vec<String>,
    pub(crate) circuit_state: String,
    /// Controls the machine exposes that this arm never attempted to write.
    pub(crate) untouched_controls: Vec<String>,
}

/// What the standalone S3D recovery pass did, run through the real
/// `recover_directory` code against the simulated machine.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct S3dRecovery {
    /// `false` when the `optid-recover` executable could not be found or run.
    pub(crate) available: bool,
    pub(crate) scanned: usize,
    pub(crate) restored: usize,
    pub(crate) already_restored: usize,
    pub(crate) relinquished: usize,
    pub(crate) failed: usize,
    pub(crate) succeeded: bool,
    /// The exit code a supervisor would observe from `optid-recover`.
    pub(crate) exit_code: i32,
    pub(crate) events: Vec<String>,
}

/// The machine-readable evidence bundle.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Bundle {
    pub(crate) schema_version: u32,
    pub(crate) question: String,
    pub(crate) claim_scope: String,
    pub(crate) simulation_root: String,
    pub(crate) repeats: u32,
    pub(crate) containment_guards: Vec<String>,
    pub(crate) total_host_write_attempts: u64,
    pub(crate) containment_violations: Vec<String>,
    pub(crate) machine: analysis::MachineSummary,
    pub(crate) assumptions: Vec<Assumptions>,
    pub(crate) arms: Vec<Arm>,
    pub(crate) scenarios: Vec<analysis::ScenarioSummary>,
    pub(crate) trials: Vec<Trial>,
    pub(crate) determinism: analysis::DeterminismReport,
    pub(crate) comparisons: Vec<analysis::Comparison>,
    pub(crate) attribution: Vec<analysis::Attribution>,
    pub(crate) masking: Vec<analysis::MaskingFinding>,
    pub(crate) controls: analysis::ControlReport,
    pub(crate) safety: analysis::SafetyReport,
    pub(crate) support: analysis::SupportReport,
    pub(crate) findings: Vec<analysis::Finding>,
    pub(crate) verdict: analysis::Verdict,
}

fn policy_toml(arm: &Arm) -> String {
    let mut out = String::new();
    out.push_str(
        "# Generated by the optid simulated-evidence harness.\n\
         # Values describe a MODELLED machine. Nothing here is a hardware claim.\n\n",
    );
    out.push_str("[policy]\nowner = \"optid\"\ncompeting_policy_daemons = []\n\n");
    out.push_str("[safety]\ncapability_sealing = \"observe\"\ncircuit_failure_threshold = 2\ncircuit_cooldown_sec = 30\n\n");
    out.push_str("[thermal]\nmode = \"observe\"\n\n");
    out.push_str("[memory]\nowner = \"optid\"\nhigh_swappiness_requires_zram = true\n\n");
    out.push_str(
        "[thresholds]\ncpu_pressure_perf_avg10 = 12.0\nmemory_pressure_protect_avg10 = 5.0\n\
         io_pressure_throttle_avg10 = 8.0\nhot_temp_c = 82.0\ncritical_temp_c = 92.0\n\
         low_battery_pct = 20\n\n",
    );
    match arm.policy {
        PolicyFlavour::Curated => {
            out.push_str(
                "[modes.battery]\ncpu_epp = \"power\"\nplatform_profile = \"low-power\"\n\
                 vm_swappiness = 60\nvm_dirty_background_bytes = 67108864\nvm_dirty_bytes = 134217728\n\n\
                 [modes.balanced]\ncpu_epp = \"balance_performance\"\nplatform_profile = \"balanced\"\n\
                 vm_swappiness = 100\nvm_dirty_background_bytes = 67108864\nvm_dirty_bytes = 134217728\n\n\
                 [modes.performance]\ncpu_epp = \"performance\"\nplatform_profile = \"performance\"\n\
                 vm_swappiness = 150\nvm_dirty_background_bytes = 67108864\nvm_dirty_bytes = 134217728\n\n\
                 [modes.realtime]\ncpu_epp = \"performance\"\nplatform_profile = \"performance\"\n\
                 vm_swappiness = 10\n\n",
            );
        }
        PolicyFlavour::Harmful => {
            // The positive control: every mode asks for the wrong thing.
            out.push_str(
                "[modes.battery]\ncpu_epp = \"performance\"\nplatform_profile = \"performance\"\n\
                 vm_swappiness = 0\nvm_dirty_background_bytes = 1048576\nvm_dirty_bytes = 2097152\n\n\
                 [modes.balanced]\ncpu_epp = \"power\"\nplatform_profile = \"low-power\"\n\
                 vm_swappiness = 0\nvm_dirty_background_bytes = 1048576\nvm_dirty_bytes = 2097152\n\n\
                 [modes.performance]\ncpu_epp = \"power\"\nplatform_profile = \"low-power\"\n\
                 vm_swappiness = 0\nvm_dirty_background_bytes = 1048576\nvm_dirty_bytes = 2097152\n\n\
                 [modes.realtime]\ncpu_epp = \"power\"\nplatform_profile = \"low-power\"\n\
                 vm_swappiness = 0\n\n",
            );
        }
    }
    out.push_str("[domains]\n");
    for (domain, mode) in &arm.domains {
        out.push_str(&format!("[domains.{domain}]\nmode = \"{mode}\"\n\n"));
    }
    out
}

fn contracts_toml(arm: &Arm) -> String {
    match arm.policy {
        PolicyFlavour::Curated => "[contracts]\nmode = \"enforce\"\n\n\
             [contracts.idle]\ncpu_wakeup_latency = 100000\ndevice_resume_latency = 1000000\n\n\
             [contracts.light]\ncpu_wakeup_latency = 50000\ndevice_resume_latency = 500000\n\n\
             [contracts.interactive]\ncpu_wakeup_latency = 1000\ndevice_resume_latency = 10000\n\n\
             [contracts.latency-critical]\ncpu_wakeup_latency = 1000\ndevice_resume_latency = 1000\n\n\
             [contracts.throughput]\ncpu_wakeup_latency = 10000\ndevice_resume_latency = 100000\n"
            .to_string(),
        // The harmful control also loosens every latency floor, so the PM QoS
        // lever asks for the worst value rather than the best one.
        PolicyFlavour::Harmful => "[contracts]\nmode = \"enforce\"\n\n\
             [contracts.idle]\ncpu_wakeup_latency = 2000000\ndevice_resume_latency = 2000000\n\n\
             [contracts.light]\ncpu_wakeup_latency = 2000000\ndevice_resume_latency = 2000000\n\n\
             [contracts.interactive]\ncpu_wakeup_latency = 2000000\ndevice_resume_latency = 2000000\n\n\
             [contracts.latency-critical]\ncpu_wakeup_latency = 2000000\ndevice_resume_latency = 2000000\n\n\
             [contracts.throughput]\ncpu_wakeup_latency = 2000000\ndevice_resume_latency = 2000000\n"
            .to_string(),
    }
}

const INVALID_POLICY: &str = "[policy\nthis file is not valid TOML = = =\n";

/// How many times the harness plays the supervisor and restarts a daemon that
/// exited non-zero. Bounded so a run cannot loop forever.
const MAX_SUPERVISED_RESTARTS: u32 = 3;

/// A daemon exit that a supervisor would restart.
fn needs_restart(result: &io::Result<crate::RunExit>) -> bool {
    match result {
        Ok(crate::RunExit::Clean) => false,
        Ok(_) => true,
        Err(_) => true,
    }
}

struct TrialPaths {
    machine_dir: PathBuf,
    state_dir: PathBuf,
    config_path: PathBuf,
}

fn prepare_trial_paths(
    root: &Path,
    arm: &Arm,
    scenario: &Scenario,
    repeat: u32,
) -> io::Result<TrialPaths> {
    let run_dir = root
        .join("runs")
        .join(format!("{}__{}__r{repeat}", arm.id, scenario.id));
    if run_dir.exists() {
        fs::remove_dir_all(&run_dir)?;
    }
    let machine_dir = run_dir.join("machine");
    let state_dir = run_dir.join("state");
    let config_dir = run_dir.join("config");
    fs::create_dir_all(&machine_dir)?;
    fs::create_dir_all(&state_dir)?;
    fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("policy.toml");
    fs::write(&config_path, policy_toml(arm))?;
    fs::write(config_dir.join("contracts.toml"), contracts_toml(arm))?;
    Ok(TrialPaths {
        machine_dir,
        state_dir,
        config_path,
    })
}

fn daemon_args(paths: &TrialPaths) -> Args {
    Args {
        apply: true,
        once: false,
        help: false,
        version: false,
        // The simulated clock, not the wall clock, drives dwell and hysteresis,
        // so the loop does not need to sleep between cycles.
        interval_sec: 0,
        state_dir: paths.state_dir.clone(),
        config_path: paths.config_path.clone(),
        allowlist: true,
        foreground: crate::args::ForegroundMode::Off,
    }
}

fn describe_run(result: &io::Result<crate::RunExit>) -> String {
    match result {
        Ok(exit) => format!("{exit:?}").to_lowercase(),
        Err(error) => format!("error: {} ({:?})", error, error.kind()),
    }
}

/// Run one arm against one scenario once.
fn run_trial(root: &Path, arm: &Arm, scenario: &Scenario, repeat: u32) -> Result<Trial, String> {
    let paths = prepare_trial_paths(root, arm, scenario, repeat)
        .map_err(|error| format!("cannot prepare {}/{}: {error}", arm.id, scenario.id))?;
    let spec = scenarios::machine_spec();

    let machine = SimMachine::new(
        paths.machine_dir.clone(),
        root.to_path_buf(),
        paths.state_dir.clone(),
        paths.config_path.clone(),
        policy_toml(arm),
        INVALID_POLICY.to_string(),
        &spec,
        scenario.env.clone(),
        scenario.workload.clone(),
        Assumptions::nominal(),
        scenario.faults.clone(),
        scenario.events.clone(),
        scenario.cycles,
        scenario.step_seconds,
        1_760_000_000,
    )
    .map_err(|error| format!("cannot build the simulated machine: {error}"))?;

    if arm.allowlist_override {
        let dir = paths.machine_dir.join("etc/optid/allowlist.d");
        fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        fs::write(
            dir.join("50-simulation.toml"),
            scenarios::simulation_allowlist_override(),
        )
        .map_err(|error| format!("cannot write the allowlist override: {error}"))?;
    }

    let baseline = machine.baseline_values();
    let mut daemon_outcome = "daemon_absent".to_string();
    let mut recovery_outcome = None;
    let mut s3d = Vec::new();

    match arm.kind {
        ArmKind::DaemonAbsent => {
            for _ in 0..scenario.cycles {
                machine.step_cycle();
            }
        }
        ArmKind::Daemon => {
            let mut result = drive_daemon(&machine, &paths);
            daemon_outcome = describe_run(&result);
            let crashed = scenario
                .faults
                .iter()
                .any(|fault| matches!(fault, SimFault::Crash { .. }));
            // A unit file restarts optid when it exits non-zero, and asks for a
            // cold restart on the topology-rebuild exit. The harness plays that
            // supervisor so the production recovery path is what decides whether
            // the machine is handed back, exactly as it would be on a real host.
            let mut restarts = 0u32;
            let mut reboot_pending = crashed;
            let mut outcomes = Vec::new();
            while restarts < MAX_SUPERVISED_RESTARTS && needs_restart(&result) {
                if reboot_pending {
                    // A crash is followed by a reboot: `/run` is a tmpfs and
                    // does not survive, while the durable recovery records
                    // under `/var/lib/optid` do.
                    let _ = fs::remove_dir_all(&paths.state_dir);
                    let _ = fs::create_dir_all(&paths.state_dir);
                    reboot_pending = false;
                }
                // `optid-apply.service` declares `Requires=optid-recover.service`
                // and `optid-recover.service` declares `PartOf=optid-apply.service`,
                // so the one-shot S3D recovery runs before every start of the
                // daemon, including an automatic restart. The harness follows
                // that ordering rather than inventing its own.
                s3d.push(run_s3d_recovery(&paths));
                machine.begin_recovery_phase(2);
                result = drive_daemon(&machine, &paths);
                outcomes.push(describe_run(&result));
                restarts += 1;
            }
            if !outcomes.is_empty() {
                recovery_outcome = Some(outcomes.join(" -> "));
            }
        }
    }

    let post = machine.control_values();
    let writes = machine.writes();
    let attempted: std::collections::BTreeSet<String> = writes
        .iter()
        .filter_map(|record| record.control_id.clone())
        .collect();
    let untouched_controls: Vec<String> = machine
        .controls()
        .values()
        .map(|control| control.id.clone())
        .filter(|id| !attempted.contains(id))
        .collect();
    let samples = machine.samples();
    let receipts = analysis::build_receipts(&machine, &baseline, &post, &writes);
    let envelope = analysis::read_envelope(&paths.state_dir);
    let circuit_state = analysis::read_circuit_state(&paths.state_dir);
    let aggregate = analysis::aggregate(&samples);
    let sensitivity = analysis::sensitivity(&samples, scenario);
    let mut rejections = analysis::validate_metrics(&aggregate, &samples);
    rejections.extend(analysis::validate_receipts(&receipts, &envelope));

    Ok(Trial {
        arm: arm.id.clone(),
        scenario: scenario.id.clone(),
        repeat,
        daemon_outcome,
        recovery_outcome,
        s3d_recovery: s3d,
        cycles_completed: machine.cycles_completed(),
        host_write_attempts: machine.host_write_attempts(),
        containment_violations: machine.violations(),
        writes,
        samples,
        receipts,
        envelope,
        aggregate,
        sensitivity,
        rejections,
        circuit_state,
        untouched_controls,
    })
}

/// The production S3D recovery directory, as the unit file names it.
const RECOVERY_DIR: &str = "/var/lib/optid/recovery";

/// Locate the `optid-recover` executable that ships beside this one. A
/// supervisor starts it by absolute path, so the harness does too and the
/// emptied `PATH` cannot be involved.
fn recover_binary() -> Option<PathBuf> {
    let candidate = std::env::current_exe()
        .ok()?
        .parent()?
        .join("optid-recover");
    candidate.is_file().then_some(candidate)
}

/// Run the real `optid-recover` binary against the simulated machine, exactly
/// as `optid-recover.service` runs it before every start of the daemon. The
/// `--machine-root` flag exists only in a `test-simulation` build and rebases
/// every recorded target path into the simulated machine tree, so no write can
/// reach a host path.
fn run_s3d_recovery(paths: &TrialPaths) -> S3dRecovery {
    let recovery_dir = paths.machine_dir.join(RECOVERY_DIR.trim_start_matches('/'));
    let status_file = paths.state_dir.join("recovery-status.json");
    let Some(binary) = recover_binary() else {
        return S3dRecovery {
            available: false,
            scanned: 0,
            restored: 0,
            already_restored: 0,
            relinquished: 0,
            failed: 0,
            succeeded: false,
            exit_code: -1,
            events: vec![
                "the optid-recover executable was not found beside this binary; the S3D step was                  not run"
                    .to_string(),
            ],
        };
    };
    let output = std::process::Command::new(&binary)
        .arg("--recovery-dir")
        .arg(&recovery_dir)
        .arg("--status-file")
        .arg(&status_file)
        .arg("--machine-root")
        .arg(&paths.machine_dir)
        .output();
    let Ok(output) = output else {
        return S3dRecovery {
            available: false,
            scanned: 0,
            restored: 0,
            already_restored: 0,
            relinquished: 0,
            failed: 0,
            succeeded: false,
            exit_code: -1,
            events: vec!["optid-recover could not be started".to_string()],
        };
    };
    let exit_code = output.status.code().unwrap_or(-1);
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    let count = |key: &str| {
        summary
            .get(key)
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as usize
    };
    let events = summary
        .get("events")
        .and_then(|value| value.as_array())
        .map(|events| {
            events
                .iter()
                .map(|event| {
                    format!(
                        "{}: {} — {}",
                        event
                            .get("target_id")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown"),
                        event
                            .get("disposition")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown"),
                        event
                            .get("detail")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    S3dRecovery {
        available: true,
        scanned: count("scanned"),
        restored: count("restored"),
        already_restored: count("already_restored"),
        relinquished: count("relinquished"),
        failed: count("failed"),
        succeeded: exit_code == 0,
        exit_code,
        events,
    }
}

fn drive_daemon(machine: &Arc<SimMachine>, paths: &TrialPaths) -> io::Result<crate::RunExit> {
    let kernel = SimKernel::new(Arc::clone(machine));
    let sink_machine = Arc::clone(machine);
    let previous = crate::actuator::set_pmqos_sink_factory(Some(Box::new(move || {
        Box::new(SimPmqosSink::new(Arc::clone(&sink_machine)))
            as Box<dyn crate::actuator::PmqosSink>
    })));
    let args = daemon_args(paths);
    let result = with_real_kernel_override(Box::new(kernel), || crate::run(args));
    crate::actuator::set_pmqos_sink_factory(previous);
    // The process is gone now. Its descriptor on `/dev/cpu_dma_latency` closes
    // with it, and the kernel drops the request — including after a crash.
    machine.release_cpu_pm_qos();
    result
}

/// Run the whole matrix and produce the evidence bundle.
pub(crate) fn run_evidence(options: &EvidenceOptions) -> Result<Bundle, String> {
    let root = validate_root(&options.root)?;
    let guards = install_process_guards(&root)?;

    let arms = scenarios::arms();
    let scenario_list = scenarios::scenarios();
    let mut trials = Vec::new();
    for repeat in 0..options.repeats {
        for arm in &arms {
            for scenario in &scenario_list {
                trials.push(run_trial(&root, arm, scenario, repeat)?);
            }
        }
    }

    let determinism = analysis::determinism(&trials, options.repeats);
    let comparisons = analysis::compare(&trials, &arms, &scenario_list);
    let attribution = analysis::attribute(&trials, &arms, &scenario_list);
    let masking = analysis::masking(&comparisons, &attribution);
    let controls = analysis::controls(&trials, &comparisons, &scenario_list);
    let safety = analysis::safety(&trials, &scenario_list);
    let support = analysis::support(&trials, &scenario_list);
    let findings = analysis::findings(&trials, &scenario_list);
    let verdict = analysis::verdict(&comparisons, &controls, &safety, &determinism, &support);

    let total_host_write_attempts = trials.iter().map(|trial| trial.host_write_attempts).sum();
    let mut containment_violations: Vec<String> = trials
        .iter()
        .flat_map(|trial| trial.containment_violations.clone())
        .collect();
    containment_violations.sort();
    containment_violations.dedup();

    Ok(Bundle {
        schema_version: BUNDLE_SCHEMA_VERSION,
        question: "When optid is fully enabled, does it theoretically improve the modelled system \
                   compared with optid off, while remaining safe under faults and recovery?"
            .to_string(),
        claim_scope:
            "SIMULATED AND MODELLED ONLY. No measurement of physical power, battery life, \
                      temperature, hardware compatibility or real-world performance is made or \
                      implied anywhere in this bundle."
                .to_string(),
        simulation_root: root.display().to_string(),
        repeats: options.repeats,
        containment_guards: guards,
        total_host_write_attempts,
        containment_violations,
        machine: analysis::machine_summary(&scenarios::machine_spec()),
        assumptions: Assumptions::grid(),
        arms,
        scenarios: scenario_list
            .iter()
            .map(analysis::scenario_summary)
            .collect(),
        trials,
        determinism,
        comparisons,
        attribution,
        masking,
        controls,
        safety,
        support,
        findings,
        verdict,
    })
}

/// Build the committable summary: the first repeat of every arm and scenario,
/// without the raw per-cycle traces.
fn summarise(bundle: &Bundle, full_bundle_bytes: usize) -> serde_json::Value {
    let mut value = serde_json::to_value(bundle).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        let trials = object
            .get("trials")
            .and_then(|trials| trials.as_array())
            .map(|trials| {
                trials
                    .iter()
                    .filter(|trial| trial.get("repeat").and_then(|r| r.as_u64()) == Some(0))
                    .map(|trial| {
                        let mut trial = trial.clone();
                        if let Some(trial) = trial.as_object_mut() {
                            trial.remove("writes");
                            trial.remove("samples");
                            trial.remove("sensitivity");
                        }
                        trial
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        object.insert("trials".to_string(), serde_json::Value::Array(trials));
        object.insert(
            "summary_note".to_string(),
            serde_json::Value::String(
                "This is the committed summary. Per-trial write logs, per-cycle modelled samples                  and per-trial sensitivity tables are omitted; the repeats used to prove                  determinism are represented by the determinism report rather than by their own                  records. Every judgement in this file was computed from the full bundle."
                    .to_string(),
            ),
        );
        object.insert(
            "full_bundle_bytes".to_string(),
            serde_json::Value::from(full_bundle_bytes),
        );
        object.insert(
            "reproduce_command".to_string(),
            serde_json::Value::String(format!(
                "cargo run --release -p optid --features test-simulation --bin optid --                  --evidence-root <marked empty directory> --evidence-repeats {}",
                bundle.repeats
            )),
        );
    }
    value
}

/// CLI entry point. Returns the process exit code.
pub(crate) fn main_entry(options: &EvidenceOptions) -> i32 {
    let bundle = match run_evidence(options) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("optid --evidence-root: {error}");
            return 1;
        }
    };
    let out_dir = options.root.join("out");
    if let Err(error) = fs::create_dir_all(&out_dir) {
        eprintln!(
            "optid --evidence-root: cannot create {}: {error}",
            out_dir.display()
        );
        return 1;
    }
    let json = match serde_json::to_string_pretty(&bundle) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("optid --evidence-root: cannot serialize the bundle: {error}");
            return 1;
        }
    };
    if let Err(error) = fs::write(out_dir.join("evidence-bundle.json"), format!("{json}\n")) {
        eprintln!("optid --evidence-root: cannot write the bundle: {error}");
        return 1;
    }

    // The full bundle carries every write and every modelled cycle for every
    // repeat, which is tens of megabytes. The summary is the same evidence with
    // the repeated runs and the raw per-cycle traces removed: it keeps the
    // receipts, the aggregates, the comparisons and every judgement. The full
    // bundle is reproducible from the summary's `reproduce_command`, and the
    // determinism report is what makes that reproduction meaningful.
    let summary = summarise(&bundle, json.len());
    let summary_json = match serde_json::to_string_pretty(&summary) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("optid --evidence-root: cannot serialize the summary: {error}");
            return 1;
        }
    };
    if let Err(error) = fs::write(
        out_dir.join("evidence-summary.json"),
        format!("{summary_json}\n"),
    ) {
        eprintln!("optid --evidence-root: cannot write the summary: {error}");
        return 1;
    }
    let markdown = report::render(&bundle);
    if let Err(error) = fs::write(out_dir.join("report.md"), markdown) {
        eprintln!("optid --evidence-root: cannot write the report: {error}");
        return 1;
    }
    println!("{}", report::render_console(&bundle));
    if bundle.verdict.blocking_failures.is_empty() {
        0
    } else {
        1
    }
}
