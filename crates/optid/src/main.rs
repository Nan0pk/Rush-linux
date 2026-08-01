//! `optid` — the Rush Linux adaptive optimization daemon.
//!
//! The daemon runs one snapshot → classify → decide → reconcile → actuate
//! control cycle. F4 makes the reconciler the sole owner of transition
//! restoration; legacy journal helpers remain migration utilities only.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use zbus::blocking::connection::Builder as ConnectionBuilder;

mod action;
// Legacy reversion helpers remain compiled for historical regression tests, but
// F4 removed every production caller. Keep them quarantined until a later
// dedicated deletion can remove the compatibility surface without obscuring
// this architectural cutover.
#[allow(dead_code)]
mod actuator;
mod actuators;
mod allowlist;
mod args;
mod capability;
mod contracts;
mod dbus;
mod decision;
mod envelope;
mod foreground;
#[allow(dead_code)]
mod io_util;
mod kernel_io;
mod load_state;
mod policy;
mod reconciler;
mod sensors;
mod shim;
#[cfg(test)]
mod tests;
mod thermal;
mod workload;

use actuator::Actuator;
use args::{
    parse_from_env, print_usage, print_version, Args, DEFAULT_DWELL_WINDOW_SEC,
    DEFAULT_MODE_DWELL_WINDOW_SEC,
};
use contracts::Contracts;
use dbus::OptidServer;
use envelope::{ActionOutcome, ControlCycleEnvelope, CycleIdGenerator};
use io_util::{append_log, append_log_with, atomic_write_state_file_with};
use kernel_io::Clock;
use load_state::{BootState, LoadState};
use policy::Policy;
use sensors::Snapshot;
use shim::{GameModeServer, PpdServer};
use workload::{
    read_global_pinned_class, read_mode_override, read_pinned_class, HysteresisState, Mode,
    ModeHysteresisState, WorkloadClass,
};

fn main() {
    let args = match parse_from_env() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("optid: {err}");
            print_usage();
            std::process::exit(2);
        }
    };

    if args.help {
        print_usage();
        return;
    }
    if args.version {
        print_version();
        return;
    }
    if let Err(err) = run(args) {
        eprintln!("optid: {err}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> io::Result<()> {
    fs::create_dir_all(&args.state_dir)?;
    let lock_file = fs::File::create(args.state_dir.join("optid.lock"))?;
    let lock_res = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if lock_res != 0 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another instance of optid is already running on this state directory",
        ));
    }

    let term = Arc::new(AtomicBool::new(false));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&term));

    let (policy_for_conflicts, policy_load_state) = Policy::load_with_state(&args.config_path);
    let conflict_report =
        shim::detect_conflicts(&policy_for_conflicts.policy.competing_policy_daemons);
    let mut args = args;
    if conflict_report.is_blocking() {
        let advice = conflict_report.render_advice();
        eprintln!("optid: {advice}");
        append_log(
            &args.state_dir.join("decisions.log"),
            &format!("optid: {advice}\n"),
        )?;
        if args.apply {
            eprintln!("optid: --apply downgraded to dry-run due to active conflicts.");
            append_log(
                &args.state_dir.join("decisions.log"),
                "optid: --apply downgraded to dry-run due to active conflicts.\n",
            )?;
            args.apply = false;
        }
    }

    let (allowlist, allowlist_load_state) = if args.allowlist {
        allowlist::Allowlist::load_with_state(allowlist::DEFAULT_OVERRIDE_DIRS)
    } else {
        (allowlist::Allowlist::seeded(), LoadState::Ok)
    };
    let allowlist_gate_enabled = args.allowlist;
    let apply_armed = args.apply
        && policy_load_state.permits_dynamic_writes()
        && (!allowlist_gate_enabled || allowlist_load_state.permits_dynamic_writes());
    let boot_state = BootState {
        policy_load_state,
        allowlist_load_state,
        apply_armed,
        baseline_armed: false,
        allowlist_gate_enabled,
    };
    let boot_summary = format!(
        "optid: boot state — {}; independent VM baseline retired into F4 complete desired state\n",
        boot_state.summary()
    );
    eprint!("{boot_summary}");
    append_log(&args.state_dir.join("decisions.log"), &boot_summary)?;
    if !apply_armed && args.apply {
        let message =
            "optid: --apply requested but apply_armed=false — all dynamic writes disabled.\n";
        eprint!("{message}");
        append_log(&args.state_dir.join("decisions.log"), message)?;
    }

    spawn_dbus_servers(
        args.state_dir.clone(),
        &policy_for_conflicts,
        conflict_report.is_blocking(),
    );

    let initial_class = fs::read_to_string(args.state_dir.join("workload_class"))
        .ok()
        .and_then(|value| WorkloadClass::parse(&value))
        .unwrap_or(WorkloadClass::Idle);
    let mut hysteresis = HysteresisState::new(initial_class);
    let mut mode_hysteresis = ModeHysteresisState::new(Mode::Balanced);

    let mut actuator = Actuator::new(args.state_dir.clone());
    if allowlist_gate_enabled {
        let mut summary = format!(
            "optid: WP-N4 hardware allowlist gate ENABLED (default-deny); version={} effective_entries={} load_state={}\n",
            allowlist.version(),
            allowlist.len(),
            allowlist_load_state,
        );
        for entry in allowlist.entries() {
            summary.push_str("optid:   allowlist ");
            summary.push_str(&entry.describe());
            summary.push('\n');
        }
        append_log(&args.state_dir.join("decisions.log"), &summary)?;
        actuator.enable_allowlist(allowlist);
    }
    actuator.set_boot_state(boot_state.clone());
    // Keep the legacy method linked for direct tests, but do not perform an
    // independent production VM write. VM/sysctl is now reconciled like every
    // other target. In dry-run this method is a no-op because baseline_armed is
    // false.
    if !args.apply {
        actuator.apply_baseline()?;
    }

    let cycle_kernel = kernel_io::RealKernel::new();
    let mut cycle_ids = CycleIdGenerator::new(cycle_kernel.now_unix());
    let mut reconciler =
        reconciler::Reconciler::load(args.state_dir.clone(), actuator.kernel.as_ref())?;
    append_log(
        &args.state_dir.join("decisions.log"),
        &format!("optid: F4 reconciler mode={:?}\n", reconciler.mode()),
    )?;

    if args.foreground == args::ForegroundMode::Auto {
        let foreground_config = policy_for_conflicts.foreground.clone();
        let _foreground_rx = foreground::subscribe(args.state_dir.clone(), foreground_config);
        append_log(
            &args.state_dir.join("decisions.log"),
            "optid: foreground detection ENABLED (--foreground=auto). v0.6 stub.\n",
        )?;
    }

    let mut previous_thermal_budget: Option<thermal::ThermalBudget> = None;
    let startup_policy = Policy::load(&args.config_path);
    append_log(
        &args.state_dir.join("decisions.log"),
        &format!(
            "optid: thermal startup mode={:?} (no baseline scan; loop uses configured policy)\n",
            startup_policy.thermal.mode
        ),
    )?;

    let mut legacy_active_keys = BTreeSet::new();
    loop {
        let override_mode = read_mode_override(&args.state_dir).unwrap_or(Mode::Auto);
        let policy = Policy::load(&args.config_path);
        let kernel = cycle_kernel.clone();
        let mut snapshot = Snapshot::collect_with_thermal(
            &kernel,
            &kernel,
            &policy.thermal,
            previous_thermal_budget.as_ref(),
        );
        previous_thermal_budget = Some(snapshot.thermal_budget.clone());
        snapshot.global_pinned_class = read_global_pinned_class(&args.state_dir);
        if let Some(ref app) = snapshot.foreground_app {
            snapshot.pinned_class = read_pinned_class(&args.state_dir, app);
        }
        let (raw_class, class_reason) = policy.classify(&snapshot);
        let (committed_class, _) =
            hysteresis.update(raw_class, snapshot.timestamp, DEFAULT_DWELL_WINDOW_SEC);
        let resolved_mode = match override_mode {
            Mode::Auto => {
                let raw_mode = policy.auto_mode(&snapshot);
                let critical_thermal = policy.is_critical_thermal(&snapshot);
                let (mode, _, _) = mode_hysteresis.update(
                    raw_mode,
                    snapshot.timestamp,
                    DEFAULT_MODE_DWELL_WINDOW_SEC,
                    critical_thermal,
                );
                mode
            }
            explicit => {
                mode_hysteresis.force(explicit);
                explicit
            }
        };
        let mode_hysteresis_reason = mode_hysteresis.explain_pending(snapshot.timestamp);
        let _ = fs::write(
            args.state_dir.join("workload_class"),
            committed_class.to_string(),
        );

        let contracts_path = args
            .config_path
            .parent()
            .map(|parent| parent.join("contracts.toml"))
            .unwrap_or_else(|| PathBuf::from("contracts.toml"));
        let contracts = Contracts::load(&contracts_path);
        actuator.set_active_floors(contracts.resolve(committed_class));
        let decision = policy.decide_resolved(
            &snapshot,
            override_mode,
            committed_class,
            class_reason,
            &contracts,
            Some(resolved_mode),
            mode_hysteresis_reason,
        );

        let correlation_id = cycle_ids.next();
        actuator.set_correlation_id(correlation_id.clone());
        let mut domain_modes = HashMap::new();
        for &domain in policy::Domain::all() {
            domain_modes.insert(domain, decision.effective_config.mode_for(domain));
        }
        for transition in reconciler.detect_transitions(
            snapshot.on_ac,
            committed_class,
            resolved_mode,
            &domain_modes,
        ) {
            append_log(
                &args.state_dir.join("decisions.log"),
                &format!(
                    "correlation_id={} reconciler: transition={}\n",
                    correlation_id,
                    transition.describe()
                ),
            )?;
        }

        let reconciled_actions: &[action::Action] =
            if apply_armed { &decision.actions } else { &[] };
        let stale_target_ids = reconciler.prepare_cycle(reconciled_actions, &mut actuator)?;
        if !stale_target_ids.is_empty() {
            append_log(
                &args.state_dir.join("decisions.log"),
                &format!(
                    "correlation_id={} reconciler: stale_targets={}\n",
                    correlation_id,
                    stale_target_ids.join(",")
                ),
            )?;
        }

        let legacy_current_keys: BTreeSet<String> = decision
            .actions
            .iter()
            .filter_map(|action| action.journal_key())
            .collect();
        let legacy_stale_keys: BTreeSet<String> = legacy_active_keys
            .difference(&legacy_current_keys)
            .cloned()
            .collect();

        let mut action_outcomes: Vec<ActionOutcome> = Vec::new();
        if apply_armed {
            for action in &decision.actions {
                action_outcomes.push(reconciler.apply_action(&mut actuator, action)?);
            }
        } else {
            for action in &decision.actions {
                action_outcomes.push(ActionOutcome::suppressed(
                    action,
                    policy::DomainMode::Actuate,
                    false,
                ));
            }
        }
        for (_, action) in &decision.suppressed_actions {
            action_outcomes.push(ActionOutcome::suppressed(
                action,
                policy::DomainMode::Observe,
                args.apply,
            ));
        }

        let parity = reconciler.parity_report(&legacy_stale_keys);
        append_log(
            &args.state_dir.join("decisions.log"),
            &format!(
                "correlation_id={} reconciler: shadow_parity={} legacy_restore_intents={} v1_restore_intents={} intentional_v1_only={}\n",
                correlation_id,
                parity.parity,
                parity.legacy.len(),
                parity.v1.len(),
                parity.intentional_v1_only.len(),
            ),
        )?;
        legacy_active_keys = legacy_current_keys;

        let restore_outcomes = reconciler.reconcile(&mut actuator)?;
        let cycle = ControlCycleEnvelope::build(
            correlation_id.clone(),
            &snapshot,
            &decision,
            &boot_state,
            action_outcomes,
            restore_outcomes.clone(),
        );
        cycle
            .validate_schema()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let status_json = cycle
            .to_pretty_json()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let cycle_line = cycle
            .to_json_line()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let restore_summary = if restore_outcomes.is_empty() {
            "none".to_string()
        } else {
            restore_outcomes
                .iter()
                .map(|outcome| format!("{}:{:?}", outcome.target_id, outcome.reason))
                .collect::<Vec<_>>()
                .join(",")
        };
        let report = format!(
            "correlation_id={}\nrestore_outcomes={}\n{}",
            correlation_id,
            restore_summary,
            decision.render(&snapshot)
        );

        atomic_write_state_file_with(&cycle_kernel, &args.state_dir.join("status"), &report)?;
        atomic_write_state_file_with(
            &cycle_kernel,
            &args.state_dir.join("status.json"),
            &status_json,
        )?;
        append_log_with(
            &cycle_kernel,
            &args.state_dir.join("control-cycles.jsonl"),
            &cycle_line,
        )?;
        append_log_with(
            &cycle_kernel,
            &args.state_dir.join("decisions.log"),
            &report,
        )?;

        if args.once || term.load(Ordering::Relaxed) {
            break;
        }
        let sleep_duration = Duration::from_secs(args.interval_sec);
        let start = Instant::now();
        while start.elapsed() < sleep_duration {
            if term.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if term.load(Ordering::Relaxed) {
            break;
        }
    }

    let shutdown_outcomes = reconciler.restore_all_owned(&mut actuator)?;
    for outcome in shutdown_outcomes {
        append_log(
            &args.state_dir.join("decisions.log"),
            &format!(
                "correlation_id={} reconciler: clean_shutdown target={} outcome={:?}\n",
                actuator.correlation_id, outcome.target_id, outcome.reason
            ),
        )?;
    }
    let _ = lock_file;
    Ok(())
}

fn spawn_dbus_servers(state_dir: PathBuf, policy: &Policy, disabled: bool) {
    let ppd_profile_map = policy.shim.ppd.profiles.clone();
    let gamemode_pin_class = policy.shim.gamemode.pin_class.clone();
    let gamemode_ttl_sec = policy.shim.gamemode.ttl_sec;
    thread::spawn(move || {
        let server = OptidServer {
            state_dir: state_dir.clone(),
        };
        let ppd_server = PpdServer::new(state_dir.clone(), ppd_profile_map);
        let gamemode_server = GameModeServer::new(state_dir, gamemode_pin_class, gamemode_ttl_sec);
        if !gamemode_server.pin_class_is_valid() {
            eprintln!("optid: GameMode shim disabled — invalid [shim.gamemode].pin_class");
        }
        let run_server = || -> zbus::Result<()> {
            let mut builder = ConnectionBuilder::system()?
                .name("io.rushlinux.Optid")?
                .serve_at("/io/rushlinux/Optid", server)?;
            if !disabled {
                builder = builder
                    .name("net.hadess.PowerProfiles")?
                    .serve_at("/net/hadess/PowerProfiles", ppd_server)?;
                if gamemode_server.pin_class_is_valid() {
                    builder = builder
                        .name("com.feralinteractive.GameMode")?
                        .serve_at("/com/feralinteractive/GameMode", gamemode_server)?;
                }
            }
            let _connection = builder.build()?;
            loop {
                thread::park();
            }
        };
        if let Err(error) = run_server() {
            eprintln!("D-Bus server error: {error}. Running without D-Bus.");
        }
    });
}
