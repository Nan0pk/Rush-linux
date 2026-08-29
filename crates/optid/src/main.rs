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
mod actuator;
mod actuators;
mod allowlist;
mod args;
mod capability;
mod capability_table;
mod circuit_breaker;
mod contracts;
mod dbus;
mod decision;
mod envelope;
mod foreground;
mod io_util;
mod kernel_io;
mod latency;
mod load_state;
mod policy;
mod reconciler;
mod sensors;
mod shim;
// I2 — deterministic simulated-evidence harness. Compiled only under the
// non-default `test-simulation` feature; a shipped optid has no such surface.
#[cfg(feature = "test-simulation")]
mod sim_evidence;
#[cfg(test)]
mod tests;
mod thermal;
mod workload;

use actuator::{Actuator, PmqosSink, RealPmqosSink};
use args::{
    parse_from_env, print_usage, print_version, Args, DEFAULT_DWELL_WINDOW_SEC,
    DEFAULT_MODE_DWELL_WINDOW_SEC,
};
use capability_table::{
    topology_fingerprint, CapabilityKernel, CapabilityTable, TopologyDebouncer, TopologyDecision,
    EXIT_TOPOLOGY_REBUILD,
};
use circuit_breaker::{
    circuit_runtime_failure_outcome, circuit_suppressed_outcome, extract_circuit_clear_request,
    CircuitBreaker, CircuitClearRequest, CircuitPermit, CircuitScope,
};
use contracts::ContractBook;
use dbus::OptidServer;
use envelope::{ActionOutcome, ControlCycleEnvelope, CycleIdGenerator, WriteOutcome};
use io_util::{append_log, append_log_with, atomic_write_state_file_with};
use kernel_io::{KernelIo, RealKernel};
use load_state::{BootState, LoadState};
use policy::{CapabilitySealingMode, Policy};
use sensors::Snapshot;
use shim::{GameModeServer, PpdServer};
use workload::{
    read_global_pinned_class, read_mode_override, read_pinned_class, HysteresisState, Mode,
    ModeHysteresisState, WorkloadClass,
};

fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    // I2 — the simulated-evidence harness is selected before the daemon's own
    // argument parser runs, and only exists in a `test-simulation` build.
    #[cfg(feature = "test-simulation")]
    match sim_evidence::extract_evidence_options(&raw_args) {
        Ok(Some(options)) => std::process::exit(sim_evidence::main_entry(&options)),
        Ok(None) => {}
        Err(error) => {
            eprintln!("optid: {error}");
            std::process::exit(2);
        }
    }
    let (clear_request, filtered_args) = match extract_circuit_clear_request(raw_args) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("optid: {error}");
            print_usage();
            std::process::exit(2);
        }
    };
    let parsed = if clear_request.is_some() {
        Args::parse(filtered_args)
    } else {
        parse_from_env()
    };
    let args = match parsed {
        Ok(args) => args,
        Err(error) => {
            eprintln!("optid: {error}");
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
    if let Some(request) = clear_request {
        if let Err(error) = clear_circuits(&args, request) {
            eprintln!("optid: {error}");
            std::process::exit(1);
        }
        return;
    }
    match run(args) {
        Ok(RunExit::Clean) => {}
        Ok(RunExit::TopologyRebuild) => std::process::exit(EXIT_TOPOLOGY_REBUILD),
        Err(error) => {
            eprintln!("optid: {error}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunExit {
    Clean,
    TopologyRebuild,
}

/// Keep the pre-F4 restoration entry points link-checked while their historical
/// regression tests are migrated. Merely taking function pointers cannot run
/// them; production restoration is owned exclusively by `Reconciler`.
fn link_retired_restore_compatibility_surface() {
    let _legacy_transition_restore: fn(
        &mut Actuator,
        &str,
    ) -> io::Result<envelope::RestoreOutcome> = Actuator::revert_key_outcome;
    let _legacy_vm_baseline: fn(&mut Actuator) -> io::Result<()> = Actuator::apply_baseline;
    let _legacy_shutdown_restores: [fn(&std::path::Path); 5] = [
        io_util::revert_sysctls,
        io_util::revert_pm_qos,
        io_util::revert_runtime_pm,
        io_util::revert_storage,
        io_util::revert_display,
    ];
}

fn clear_circuits(args: &Args, request: CircuitClearRequest) -> io::Result<()> {
    if args.apply {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "circuit clearing cannot be combined with --apply",
        ));
    }
    let policy = Policy::load(&args.config_path);
    let path = CircuitBreaker::state_path_for(&args.state_dir);
    let mut breaker = CircuitBreaker::load(
        path,
        policy.safety.circuit_failure_threshold,
        policy.safety.circuit_cooldown_sec,
    );
    let effective_uid = unsafe { libc::geteuid() };
    let removed = match request {
        CircuitClearRequest::All => breaker.clear_all(effective_uid)?,
        CircuitClearRequest::Domain(domain) => {
            if !policy::Domain::all()
                .iter()
                .any(|known| known.as_str() == domain)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown circuit domain: {domain}"),
                ));
            }
            breaker.clear_domain(&domain, effective_uid)?
        }
    };
    println!("optid: cleared {removed} S5D circuit record(s)");
    Ok(())
}

fn run(args: Args) -> io::Result<RunExit> {
    link_retired_restore_compatibility_surface();
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
        allowlist::Allowlist::load_with_state(&RealKernel::new(), allowlist::DEFAULT_OVERRIDE_DIRS)
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

    let initial_class = fs::read_to_string(args.state_dir.join("workload_class"))
        .ok()
        .and_then(|value| WorkloadClass::parse(&value))
        .unwrap_or(WorkloadClass::Idle);
    let mut hysteresis = HysteresisState::new(initial_class);
    let mut mode_hysteresis = ModeHysteresisState::new(Mode::Balanced);

    let discovery_kernel = RealKernel::new();
    let startup_snapshot = Snapshot::collect_with_thermal(
        &discovery_kernel,
        &discovery_kernel,
        &policy_for_conflicts.thermal,
        None,
    );
    let startup_topology = topology_fingerprint(&discovery_kernel, &startup_snapshot);
    let mut topology_debouncer = TopologyDebouncer::new(startup_topology);

    let mut actuator_kernel: Box<dyn KernelIo> = Box::new(RealKernel::new());
    let mut cycle_kernel: Box<dyn KernelIo> = Box::new(RealKernel::new());
    let mut pmqos_sink: Box<dyn PmqosSink> = Box::new(RealPmqosSink::new());
    #[cfg(feature = "test-simulation")]
    if let Some(sink) = actuator::overridden_pmqos_sink() {
        pmqos_sink = sink;
    }
    let mut capability_sealing_enforced = false;
    #[cfg(any(test, feature = "test-simulation"))]
    let injected_kernel_test_seam = kernel_io::real_kernel_override_is_active();
    #[cfg(not(any(test, feature = "test-simulation")))]
    let injected_kernel_test_seam = false;

    if apply_armed
        && policy_for_conflicts.safety.capability_sealing == CapabilitySealingMode::Enforce
    {
        match (
            CapabilityTable::from_snapshot(&discovery_kernel, &startup_snapshot),
            RealPmqosSink::preopen_for_sealing(),
        ) {
            (Ok(table), Ok(sealed_pmqos)) => {
                let table = Arc::new(table);
                let state_roots = vec![args.state_dir.clone(), PathBuf::from("/var/lib/optid")];
                match table.seal(&state_roots) {
                    Ok(report) => {
                        let sealed_kernel = CapabilityKernel::new(Arc::clone(&table));
                        actuator_kernel = Box::new(sealed_kernel.clone());
                        cycle_kernel = Box::new(sealed_kernel);
                        pmqos_sink = Box::new(sealed_pmqos);
                        capability_sealing_enforced = true;
                        let message = format!(
                            "optid: S4D seal enforced — capabilities={} Landlock ABI={} rights=0x{:x} new_write_open_denied={} state_write_allowed={}\n",
                            report.capability_count,
                            report.landlock_abi,
                            report.handled_rights,
                            report.new_hardware_write_open_denied,
                            report.state_write_allowed,
                        );
                        eprint!("{message}");
                        append_log(&args.state_dir.join("decisions.log"), &message)?;
                    }
                    Err(error) => {
                        let message = format!(
                            "optid: S4D enforce requested but sealing failed; kernel writes remain observe-only: {error}\n"
                        );
                        eprint!("{message}");
                        let _ = append_log(&args.state_dir.join("decisions.log"), &message);
                    }
                }
            }
            (Err(error), _) => {
                let message = format!(
                    "optid: S4D capability-table construction failed; kernel writes remain observe-only: {error}\n"
                );
                eprint!("{message}");
                append_log(&args.state_dir.join("decisions.log"), &message)?;
            }
            (_, Err(error)) => {
                let message = format!(
                    "optid: S4D CPU PM QoS pre-open failed; kernel writes remain observe-only: {error}\n"
                );
                eprint!("{message}");
                append_log(&args.state_dir.join("decisions.log"), &message)?;
            }
        }
    } else {
        let reason = if !apply_armed {
            "apply is not armed"
        } else {
            "[safety].capability_sealing=observe"
        };
        let message = format!(
            "optid: S4D capability sealing observe-only ({reason}); non-systemd kernel writes suppressed\n"
        );
        eprint!("{message}");
        append_log(&args.state_dir.join("decisions.log"), &message)?;
    }

    // Binary-crate tests inject a deterministic RealKernel facade. The test-only
    // seam preserves legacy transaction tests without changing release behavior.
    if injected_kernel_test_seam {
        capability_sealing_enforced = true;
    }

    let mut actuator = Actuator::new_with_kernel(args.state_dir.clone(), actuator_kernel);
    actuator.pmqos_sink = pmqos_sink;
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
    actuator.set_capability_sealing_enforced(capability_sealing_enforced);

    let mut cycle_ids = CycleIdGenerator::new(cycle_kernel.now_unix());
    let mut reconciler = reconciler::Reconciler::load(args.state_dir.clone(), &mut actuator)?;
    append_log(
        &args.state_dir.join("decisions.log"),
        &format!("optid: F4 reconciler mode={:?}\n", reconciler.mode()),
    )?;

    let circuit_state_path = CircuitBreaker::state_path_for(&args.state_dir);
    let mut circuit_breaker = CircuitBreaker::load(
        circuit_state_path,
        policy_for_conflicts.safety.circuit_failure_threshold,
        policy_for_conflicts.safety.circuit_cooldown_sec,
    );
    if let Some(warning) = circuit_breaker.startup_warning() {
        let message = format!(
            "optid: S5D fail-closed startup warning; all actuation observe-only: {warning}\n"
        );
        eprint!("{message}");
        append_log(&args.state_dir.join("decisions.log"), &message)?;
    }
    append_log(
        &args.state_dir.join("decisions.log"),
        &format!("optid: S5D circuit state — {}\n", circuit_breaker.summary()),
    )?;

    spawn_dbus_servers(
        args.state_dir.clone(),
        &policy_for_conflicts,
        conflict_report.is_blocking(),
    );

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
        let mut snapshot = Snapshot::collect_with_thermal(
            cycle_kernel.as_ref(),
            cycle_kernel.as_ref(),
            &policy.thermal,
            previous_thermal_budget.as_ref(),
        );
        previous_thermal_budget = Some(snapshot.thermal_budget.clone());

        match topology_debouncer.observe(topology_fingerprint(cycle_kernel.as_ref(), &snapshot)) {
            TopologyDecision::Stable => {}
            TopologyDecision::Pending { observations } => {
                append_log(
                    &args.state_dir.join("decisions.log"),
                    &format!(
                        "optid: S4D topology change pending observations={observations}; new targets remain observe-only
"
                    ),
                )?;
            }
            TopologyDecision::Rebuild => {
                append_log(
                    &args.state_dir.join("decisions.log"),
                    "optid: S4D stable topology change; handing back owned targets before cold rebuild
",
                )?;
                let handbacks = reconciler.restore_all_owned(&mut actuator)?;
                for outcome in handbacks {
                    append_log(
                        &args.state_dir.join("decisions.log"),
                        &format!(
                            "optid: S4D topology handback target={} outcome={:?}
",
                            outcome.target_id, outcome.reason
                        ),
                    )?;
                }
                append_log(
                    &args.state_dir.join("decisions.log"),
                    "optid: S4D handback complete; requesting supervisor capability-table rebuild status=75
",
                )?;
                return Ok(RunExit::TopologyRebuild);
            }
        }
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
        let contract_book = ContractBook::load(&contracts_path);
        actuator.set_contracts_mode(contract_book.mode());
        // C1: compose the class contract with every override naming an active
        // cgroup scope. Scope discovery is package O2; until it lands there
        // are no active scopes and this reduces to the class row. The parsed
        // overrides are still validated and surfaced by `ContractBook`.
        let active_scopes: Vec<String> = Vec::new();
        actuator.set_active_floors(contract_book.effective_floors(committed_class, &active_scopes));
        let contracts = contract_book.base();
        let decision = policy.decide_resolved(
            &snapshot,
            override_mode,
            committed_class,
            class_reason,
            contracts,
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

        let mut circuit_plans = Vec::new();
        let mut circuit_suppressed = Vec::new();
        if apply_armed {
            for action in &decision.actions {
                let scope = CircuitScope::from_action(action, cycle_kernel.as_ref());
                match circuit_breaker.decide(&scope, snapshot.timestamp) {
                    Ok(circuit_decision)
                        if matches!(
                            circuit_decision.permit,
                            CircuitPermit::Normal | CircuitPermit::Canary
                        ) =>
                    {
                        circuit_plans.push((action.clone(), scope, circuit_decision.permit));
                    }
                    Ok(circuit_decision) => {
                        circuit_suppressed
                            .push(circuit_suppressed_outcome(action, &circuit_decision.detail));
                    }
                    Err(error) => {
                        let detail = format!(
                            "S5D circuit persistence failed; all actuation observe-only: {error}"
                        );
                        let _ = circuit_breaker.trip_global(detail.clone(), snapshot.timestamp);
                        reconciler.mark_all_for_restore();
                        circuit_suppressed.push(circuit_suppressed_outcome(action, &detail));
                    }
                }
            }
        }

        let reconciled_actions: Vec<action::Action> = circuit_plans
            .iter()
            .map(|(action, _, _)| action.clone())
            .collect();
        let stale_target_ids = match reconciler.prepare_cycle(&reconciled_actions, &mut actuator) {
            Ok(stale) => stale,
            Err(error) => {
                let detail = format!(
                    "S5D unisolatable prepare-cycle failure; global circuit opened: {error}"
                );
                let _ = circuit_breaker.trip_global(detail, snapshot.timestamp);
                reconciler.mark_all_for_restore();
                return Err(error);
            }
        };
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

        let legacy_current_keys: BTreeSet<String> = reconciled_actions
            .iter()
            .filter_map(|action| action.journal_key())
            .collect();
        let legacy_stale_keys: BTreeSet<String> = legacy_active_keys
            .difference(&legacy_current_keys)
            .cloned()
            .collect();

        let mut action_outcomes: Vec<ActionOutcome> = Vec::new();
        let mut opened_domains = BTreeSet::new();
        if apply_armed {
            for (action, scope, permit) in circuit_plans {
                if opened_domains.contains(&scope.domain) || circuit_breaker.is_global_open() {
                    let detail = if circuit_breaker.is_global_open() {
                        "S5D global circuit opened during this cycle"
                    } else {
                        "S5D domain circuit opened earlier in this cycle"
                    };
                    action_outcomes.push(circuit_suppressed_outcome(&action, detail));
                    continue;
                }

                let (outcome, transition) = match reconciler.apply_action(&mut actuator, &action) {
                    Ok(outcome) => {
                        let transition = circuit_breaker.observe_outcome(
                            &scope,
                            permit,
                            &outcome,
                            snapshot.timestamp,
                        );
                        (outcome, transition)
                    }
                    Err(error) => {
                        let outcome = circuit_runtime_failure_outcome(&action, &error);
                        let transition = circuit_breaker.record_runtime_error(
                            &scope,
                            permit,
                            &error,
                            snapshot.timestamp,
                        );
                        (outcome, transition)
                    }
                };

                match transition {
                    Ok(transition) => {
                        append_log(
                            &args.state_dir.join("decisions.log"),
                            &format!(
                                "correlation_id={} circuit: {}\n",
                                correlation_id, transition.detail
                            ),
                        )?;
                        if transition.opened {
                            opened_domains.insert(scope.domain.clone());
                            reconciler.mark_domain_for_restore(&scope.domain);
                        }
                    }
                    Err(error) => {
                        let detail = format!(
                            "S5D could not persist circuit transition; global circuit opened: {error}"
                        );
                        let _ = circuit_breaker.trip_global(detail.clone(), snapshot.timestamp);
                        reconciler.mark_all_for_restore();
                        append_log(
                            &args.state_dir.join("decisions.log"),
                            &format!("correlation_id={} circuit: {detail}\n", correlation_id),
                        )?;
                    }
                }
                action_outcomes.push(outcome);
            }
            action_outcomes.extend(circuit_suppressed);
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

        let restore_outcomes = match reconciler.reconcile(&mut actuator) {
            Ok(outcomes) => outcomes,
            Err(error) => {
                let detail = format!(
                    "S5D unisolatable reconciliation failure; global circuit opened: {error}"
                );
                let _ = circuit_breaker.trip_global(detail, snapshot.timestamp);
                return Err(error);
            }
        };
        let mut restore_failure_observed = false;
        for restore in &restore_outcomes {
            if !matches!(
                restore.write_outcome,
                WriteOutcome::RestorationFailed { .. }
            ) {
                continue;
            }
            restore_failure_observed = true;
            let Some(domain) = reconciler.domain_for_target(&restore.target_id) else {
                let detail = format!(
                    "S5D could not isolate restore failure for target {}; global circuit opened",
                    restore.target_id
                );
                let _ = circuit_breaker.trip_global(detail, snapshot.timestamp);
                reconciler.mark_all_for_restore();
                continue;
            };
            let domain = domain.to_string();
            let scope = CircuitScope::for_restore(
                domain.clone(),
                restore.target_id.clone(),
                cycle_kernel.as_ref(),
            );
            match circuit_breaker.observe_restore_outcome(&scope, restore, snapshot.timestamp) {
                Ok(transition) => {
                    append_log(
                        &args.state_dir.join("decisions.log"),
                        &format!(
                            "correlation_id={} circuit: {}
",
                            correlation_id, transition.detail
                        ),
                    )?;
                    if transition.opened {
                        reconciler.mark_domain_for_restore(&domain);
                    }
                }
                Err(error) => {
                    let detail = format!(
                        "S5D could not persist restore failure; global circuit opened: {error}"
                    );
                    let _ = circuit_breaker.trip_global(detail, snapshot.timestamp);
                    reconciler.mark_all_for_restore();
                }
            }
        }
        if !restore_failure_observed && !circuit_breaker.is_global_open() {
            if let Err(error) = circuit_breaker.mark_recovery_success(snapshot.timestamp) {
                let detail = format!(
                    "S5D recovery-success persistence failed; global circuit opened: {error}"
                );
                let _ = circuit_breaker.trip_global(detail, snapshot.timestamp);
                return Err(error);
            }
        }
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
        let circuit_status_json = circuit_breaker.public_json()?;
        let report = format!(
            "correlation_id={}\nrestore_outcomes={}\ncircuit_state={}\n{}",
            correlation_id,
            restore_summary,
            circuit_breaker.summary(),
            decision.render(&snapshot)
        );

        atomic_write_state_file_with(
            cycle_kernel.as_ref(),
            &args.state_dir.join("status"),
            &report,
        )?;
        atomic_write_state_file_with(
            cycle_kernel.as_ref(),
            &args.state_dir.join("status.json"),
            &status_json,
        )?;
        atomic_write_state_file_with(
            cycle_kernel.as_ref(),
            &args.state_dir.join("circuits.json"),
            &circuit_status_json,
        )?;
        append_log_with(
            cycle_kernel.as_ref(),
            &args.state_dir.join("control-cycles.jsonl"),
            &cycle_line,
        )?;
        append_log_with(
            cycle_kernel.as_ref(),
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
    Ok(RunExit::Clean)
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
