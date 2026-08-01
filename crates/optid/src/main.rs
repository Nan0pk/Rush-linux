//! `optid` — the Rush Linux adaptive optimization daemon.
//!
//! See `docs/SPEC-northstar.md` for the canonical objective this daemon serves
//! and `docs/adaptive-engine.md` for the high-level control loop. This file is
//! the thin entry point: it parses CLI args, starts the D-Bus server, and runs
//! the snapshot → classify → decide → actuate loop. All substantive logic lives
//! in the sibling modules so each can be reviewed and tested in isolation.

use std::collections::HashSet;
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
use io_util::{
    append_log, append_log_with, atomic_write_state_file_with, revert_display, revert_pm_qos,
    revert_runtime_pm, revert_storage, revert_sysctls,
};
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

    // Single-instance exclusive lock on state_dir/optid.lock (M4)
    let lock_file = fs::File::create(args.state_dir.join("optid.lock"))?;
    let lock_res = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if lock_res != 0 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another instance of optid is already running on this state directory",
        ));
    }

    // Register signal hooks for clean termination (H2)
    let term = Arc::new(AtomicBool::new(false));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&term));

    // v0.6 Phase B3: conflict detection. optid is the single owner of hardware
    // policy (ADR 0004); if tlp / tuned / power-profiles-daemon is already
    // running, --apply would fight them. Load the policy's competing daemon
    // list, check systemd for active instances, and downgrade --apply to
    // dry-run with a logged reason when conflicts are present. The check
    // fails OPEN (no conflicts) if systemctl is unavailable, so the daemon
    // can still start in containers and non-systemd environments.
    //
    // optid-safety: load the policy with explicit LoadState tracking. A
    // missing/malformed policy returns LoadState::Defaulted/Partial/Invalid
    // and the BootState computation below disarms apply_armed accordingly.
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

    // optid-safety: load the hardware allowlist with explicit LoadState. The
    // allowlist gate is consulted by BootState: if it loaded partially
    // (malformed override file), apply_armed is disarmed even if the policy
    // loaded cleanly.
    let (allowlist, allowlist_load_state) = if args.allowlist {
        allowlist::Allowlist::load_with_state(allowlist::DEFAULT_OVERRIDE_DIRS)
    } else {
        // Gate disabled via --no-allowlist. The load state is not consulted
        // for apply_armed (see BootState computation below).
        (allowlist::Allowlist::seeded(), LoadState::Ok)
    };

    // optid-safety: compute the boot-time decision surface. This is the
    // single source of truth for whether dynamic writes (apply_armed) and
    // curated-baseline writes (baseline_armed) are permitted.
    let allowlist_gate_enabled = args.allowlist;
    let apply_armed = args.apply
        && policy_load_state.permits_dynamic_writes()
        && (!allowlist_gate_enabled || allowlist_load_state.permits_dynamic_writes());
    let baseline_armed = args.apply; // baseline is safe by construction; only dry-run disarms
    let boot_state = BootState {
        policy_load_state,
        allowlist_load_state,
        apply_armed,
        baseline_armed,
        allowlist_gate_enabled,
    };
    let boot_summary = format!("optid: boot state — {}\n", boot_state.summary());
    eprint!("{boot_summary}");
    append_log(&args.state_dir.join("decisions.log"), &boot_summary)?;
    if !apply_armed && args.apply {
        // We asked for --apply but the gate disarmed us. Log prominently.
        let msg = format!(
            "optid: --apply requested but apply_armed=false — dynamic writes disabled. \
             Curated baseline will still be applied (baseline_armed={}).\n",
            baseline_armed
        );
        eprintln!("{msg}");
        append_log(&args.state_dir.join("decisions.log"), &msg)?;
    }

    // Revert sysctls on startup to clean up any left-over state
    revert_sysctls(&args.state_dir);
    revert_pm_qos(&args.state_dir);
    revert_runtime_pm(&args.state_dir);
    revert_storage(&args.state_dir);
    revert_display(&args.state_dir);

    let state_dir_clone = args.state_dir.clone();
    // v0.6 Phase B1: clone the PPD profile map for the D-Bus thread. The
    // map is read from policy.toml at startup; runtime changes to the file
    // are not picked up by the shim (the daemon must be restarted). This
    // matches PPD's behavior — its config is also load-on-startup.
    let ppd_profile_map = policy_for_conflicts.shim.ppd.profiles.clone();
    // v0.6 Phase B2: clone the GameMode config (pin class + TTL) for the
    // D-Bus thread. Same load-on-startup semantics as PPD.
    let gamemode_pin_class = policy_for_conflicts.shim.gamemode.pin_class.clone();
    let gamemode_ttl_sec = policy_for_conflicts.shim.gamemode.ttl_sec;
    // v0.6 Phase B1+B2: skip registering shims when a conflict is
    // detected. Attempting to claim the bus names would fail and the
    // conflict report already advised the operator to mask the
    // conflicting daemons. We also skip when `--apply` was downgraded
    // due to *any* conflict, since the shims' writes to state_dir would
    // be ignored by a dry-run daemon.
    let shims_disabled = conflict_report.is_blocking();
    thread::spawn(move || {
        let server = OptidServer {
            state_dir: state_dir_clone.clone(),
        };
        let ppd_server = PpdServer::new(state_dir_clone.clone(), ppd_profile_map);
        let gamemode_server = GameModeServer::new(
            state_dir_clone.clone(),
            gamemode_pin_class,
            gamemode_ttl_sec,
        );
        // v0.6 Phase B2: fail fast on a misconfigured pin_class.
        if !gamemode_server.pin_class_is_valid() {
            eprintln!(
                "optid: GameMode shim disabled — [shim.gamemode].pin_class in policy.toml \
                 is not a valid workload class. Edit the file and restart optid."
            );
        }
        let run_server = || -> zbus::Result<()> {
            let mut builder = ConnectionBuilder::system()?
                .name("io.rushlinux.Optid")?
                .serve_at("/io/rushlinux/Optid", server)?;
            if !shims_disabled {
                // v0.6 Phase B1: register the PPD shim.
                builder = builder
                    .name("net.hadess.PowerProfiles")?
                    .serve_at("/net/hadess/PowerProfiles", ppd_server)?;
                // v0.6 Phase B2: register the GameMode shim. Only register
                // if the pin_class config validated.
                if gamemode_server.pin_class_is_valid() {
                    builder = builder
                        .name("com.feralinteractive.GameMode")?
                        .serve_at("/com/feralinteractive/GameMode", gamemode_server)?;
                    println!(
                        "D-Bus server running on system bus at /io/rushlinux/Optid, \
                         /net/hadess/PowerProfiles (PPD shim), and \
                         /com/feralinteractive/GameMode (GameMode shim)"
                    );
                } else {
                    println!(
                        "D-Bus server running on system bus at /io/rushlinux/Optid \
                         and /net/hadess/PowerProfiles (PPD shim). \
                         GameMode shim skipped (invalid pin_class)."
                    );
                }
            } else {
                eprintln!(
                    "optid: compatibility shims disabled — conflicting daemon(s) active. \
                     Mask power-profiles-daemon.service (and/or gamemoded.service) and \
                     restart optid to enable."
                );
                println!("D-Bus server running on system bus at /io/rushlinux/Optid");
            }
            let _conn = builder.build()?;
            loop {
                thread::park();
            }
        };
        if let Err(e) = run_server() {
            eprintln!("D-Bus server error: {e}. Running without D-Bus.");
        }
    });

    let initial_class = fs::read_to_string(args.state_dir.join("workload_class"))
        .ok()
        .and_then(|s| WorkloadClass::parse(&s))
        .unwrap_or(WorkloadClass::Idle);
    let mut hysteresis = HysteresisState::new(initial_class);
    let mut mode_hysteresis = ModeHysteresisState::new(Mode::Balanced);

    let mut actuator = Actuator::new(args.state_dir.clone());
    if allowlist_gate_enabled {
        // WP-N4: the allowlist was already loaded above (with LoadState
        // tracking). Record the effective allowlist at startup so the audit
        // trail shows exactly what the gate will admit (default-deny for
        // everything else).
        let mut summary = format!(
            "optid: WP-N4 hardware allowlist gate ENABLED (default-deny); \
             version={} effective_entries={} load_state={}\n",
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

    // optid-safety: install the boot-time decision surface on the actuator.
    // After this call, every dynamic Action is gated by boot_state.apply_armed
    // (see Actuator::dynamic_writes_armed), and the curated baseline is gated
    // by boot_state.baseline_armed (see Actuator::apply_baseline).
    actuator.set_boot_state(boot_state.clone());

    // optid-safety: apply the curated baseline once at startup. This puts
    // the system into a known-good state (vm.swappiness = balanced default)
    // regardless of whether dynamic writes are armed. The baseline is gated
    // by baseline_armed, so it is skipped in dry-run mode.
    actuator.apply_baseline()?;

    // F3: one deterministic boot-scoped sequence supplies one correlation ID
    // per real control-loop iteration. The injected F2 clock keeps tests
    // deterministic without adding a UUID dependency.
    let cycle_kernel = kernel_io::RealKernel::new();
    let mut cycle_ids = CycleIdGenerator::new(cycle_kernel.now_unix());

    // v0.6 Phase C1: foreground-app detection. When --foreground=auto,
    // spawn the subscriber thread. In v0.6 this is a stub that never
    // yields events; v0.7 will fill in real compositor integration.
    if args.foreground == args::ForegroundMode::Auto {
        let fg_config = policy_for_conflicts.foreground.clone();
        let _foreground_rx = foreground::subscribe(args.state_dir.clone(), fg_config);
        append_log(
            &args.state_dir.join("decisions.log"),
            "optid: foreground detection ENABLED (--foreground=auto). \
             v0.6 stub — real compositor integration lands in v0.7.\n",
        )?;
        eprintln!(
            "optid: foreground detection enabled (--foreground=auto). \
             v0.6 STUB: no compositor integration yet; the subscriber thread \
             is idle. See FINAL-AUDIT-REPORT.md section 4.4."
        );
    }

    // Journal keys applied by the previous decision tick. Compared against
    // the current tick's keys so actions that disappear after a context
    // change are reverted rather than left applied until shutdown.
    let mut active_keys: HashSet<String> = HashSet::new();

    // T1 — retain previous thermal budget across loop iterations for
    // hysteresis (no globals). Config reloads affect future calculations
    // because Policy::load runs each tick and feeds ThermalConfig in.
    let mut previous_thermal_budget: Option<thermal::ThermalBudget> = None;

    // Post-#337 repair: the unconditional thermal baseline scan was
    // removed. The previous code called `Snapshot::collect()` (which
    // uses `ThermalConfig::default()` → mode=Observe) before loading or
    // applying the configured thermal mode, bypassing operator
    // configuration. When `[thermal] mode = "off"`, the daemon must
    // perform no thermal sysfs or legacy thermal-zone reads at startup
    // or in the loop. The loop below uses `Snapshot::collect_with_thermal`
    // with the configured policy's thermal config, which already
    // short-circuits discovery when mode=off (see `thermal.rs` and the
    // `t1_production_pipeline_off_mode_zero_thermal_reads` test).
    //
    // The startup diagnostic below logs the *configured* thermal mode
    // without performing any thermal sysfs reads. This preserves useful
    // startup visibility (the operator can see what mode optid will run
    // in) without bypassing the operator's `mode = "off"` choice.
    {
        let startup_policy = Policy::load(&args.config_path);
        let thermal_mode = startup_policy.thermal.mode;
        append_log(
            &args.state_dir.join("decisions.log"),
            &format!(
                "optid: thermal startup mode={:?} (no baseline scan; loop uses configured policy)\n",
                thermal_mode
            ),
        )?;
    }

    // F4 shadow: the reconciler tracks target-set parity (which journal
    // keys the current decision desires vs the previous tick) and
    // detects transitions for operator-visible logging. It does NOT
    // plan or apply restores — production restore still uses
    // `active_keys + Actuator::revert_key` (see below). Shadow mode is
    // honestly dormant; a future F4 cutover will wire target state from
    // journal observations and reintroduce a target-based restore
    // planner. See `crates/optid/src/reconciler.rs` module docs.
    let mut reconciler = reconciler::Reconciler::new();

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
            .map(|p| p.join("contracts.toml"))
            .unwrap_or_else(|| PathBuf::from("contracts.toml"));
        let contracts = Contracts::load(&contracts_path);

        // SPEC §3: install this tick's contract floors on the actuator
        // before applying the decision, so depth-enabler writes are gated
        // by the class the daemon actually committed to.
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

        // F4 shadow: detect transitions for operator-visible logging and
        // track target-set parity. The reconciler does NOT plan or apply
        // restores — production restore still uses `active_keys +
        // Actuator::revert_key` (see below). Shadow mode is honestly
        // dormant; see `crates/optid/src/reconciler.rs` module docs.
        {
            let mut domain_modes = std::collections::HashMap::new();
            for &d in policy::Domain::all() {
                domain_modes.insert(d, decision.effective_config.mode_for(d));
            }
            let transitions = reconciler.detect_transitions(
                snapshot.on_ac,
                committed_class,
                resolved_mode,
                &domain_modes,
            );
            // Log transitions for operator visibility. The reconciler
            // does not act on them; this is a diagnostic surface so
            // operators can see context changes that the production
            // restore path (active_keys + Actuator::revert_key) handles.
            for t in &transitions {
                append_log(
                    &args.state_dir.join("decisions.log"),
                    &format!(
                        "correlation_id={} reconciler_shadow: transition={} (production restore uses active_keys)\n",
                        correlation_id,
                        t.describe()
                    ),
                )?;
            }
            // Replace the per-tick desired set. This mirrors the
            // active_keys set replacement below and keeps shadow and
            // production tracking the same target set for equivalent
            // decisions. Actions whose journal_key() is None (e.g.
            // SystemdSetProperty, which has no property-level
            // restoration) are excluded from tracking — the reconciler
            // must not pretend to restore them.
            let mut desired_by_key: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for action in &decision.actions {
                if let Some(key) = action.journal_key() {
                    desired_by_key.insert(key, action.describe());
                }
            }
            reconciler.replace_desired_targets(&desired_by_key);

            // Log the stale target set (desired on the previous tick,
            // absent from the current decision) so operators can see
            // which targets the production restore path
            // (active_keys + Actuator::revert_key) will revert this
            // tick. This is shadow target-set comparison, not a
            // reconciler-owned restore plan.
            let stale = reconciler.stale_target_ids();
            if !stale.is_empty() {
                append_log(
                    &args.state_dir.join("decisions.log"),
                    &format!(
                        "correlation_id={} reconciler_shadow: stale_targets={} (production restore still uses active_keys)\n",
                        correlation_id,
                        stale.len()
                    ),
                )?;
            }
        }

        let mut action_outcomes: Vec<ActionOutcome> = Vec::new();
        if args.apply {
            for action in &decision.actions {
                action_outcomes.push(actuator.apply(action)?);
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

        let mut restore_outcomes = Vec::new();
        if args.apply {
            // Inverse restoration remains the existing active_keys path. F3
            // records the result returned by the actual restoration call; it
            // does not move ownership into the dormant reconciler.
            let new_keys: HashSet<String> = decision
                .actions
                .iter()
                .filter_map(|action| action.journal_key())
                .collect();
            for stale in active_keys.difference(&new_keys) {
                restore_outcomes.push(actuator.revert_key_outcome(stale)?);
            }
            active_keys = new_keys;
        }

        let cycle = ControlCycleEnvelope::build(
            correlation_id.clone(),
            &snapshot,
            &decision,
            &boot_state,
            action_outcomes,
            restore_outcomes,
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
        let report = format!(
            "correlation_id={}\n{}",
            correlation_id,
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

        let sleep_dur = Duration::from_secs(args.interval_sec);
        let start = Instant::now();
        while start.elapsed() < sleep_dur {
            if term.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if term.load(Ordering::Relaxed) {
            break;
        }
    }

    // Also revert sysctls on clean exit
    revert_sysctls(&args.state_dir);
    revert_pm_qos(&args.state_dir);
    revert_runtime_pm(&args.state_dir);
    revert_storage(&args.state_dir);
    revert_display(&args.state_dir);
    let _ = lock_file;

    Ok(())
}
