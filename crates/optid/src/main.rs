//! `optid` — the Rush Linux adaptive optimization daemon.
//!
//! See `docs/SPEC-northstar.md` for the canonical objective this daemon serves
//! and `docs/adaptive-engine.md` for the high-level control loop. This file is
//! the thin entry point: it parses CLI args, starts the D-Bus server, and runs
//! the snapshot → classify → decide → actuate loop. All substantive logic lives
//! in the sibling modules so each can be reviewed and tested in isolation.

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
mod contracts;
mod dbus;
mod decision;
mod io_util;
mod policy;
mod sensors;
mod shim;
#[cfg(test)]
mod tests;
mod workload;

use actuator::Actuator;
use args::{
    parse_from_env, print_usage, Args, DEFAULT_DWELL_WINDOW_SEC, DEFAULT_MODE_DWELL_WINDOW_SEC,
};
use contracts::Contracts;
use dbus::OptidServer;
use io_util::{
    append_log, revert_display, revert_pm_qos, revert_runtime_pm, revert_storage, revert_sysctls,
};
use policy::Policy;
use sensors::Snapshot;
use shim::PpdServer;
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
    let policy_for_conflicts = Policy::load(&args.config_path);
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
    // v0.6 Phase B1: skip registering the PPD shim when PPD is detected
    // as a conflicting daemon. Attempting to claim the
    // `net.hadess.PowerProfiles` bus name would fail and the conflict
    // report already advised the operator to mask PPD. We also skip when
    // `--apply` was downgraded due to *any* conflict, since the shim's
    // writes to state_dir/mode would be ignored by a dry-run daemon.
    let ppd_shim_disabled = conflict_report.is_blocking();
    thread::spawn(move || {
        let server = OptidServer {
            state_dir: state_dir_clone.clone(),
        };
        let ppd_server = PpdServer::new(state_dir_clone.clone(), ppd_profile_map);
        let run_server = || -> zbus::Result<()> {
            let mut builder = ConnectionBuilder::system()?
                .name("io.rushlinux.Optid")?
                .serve_at("/io/rushlinux/Optid", server)?;
            if !ppd_shim_disabled {
                // Register the PPD shim only when no PPD conflict was
                // detected. The second `name` call claims the
                // `net.hadess.PowerProfiles` bus name so PPD clients
                // (GNOME Settings, KDE powerdevil) find us.
                builder = builder
                    .name("net.hadess.PowerProfiles")?
                    .serve_at("/net/hadess/PowerProfiles", ppd_server)?;
                println!(
                    "D-Bus server running on system bus at /io/rushlinux/Optid \
                     and /net/hadess/PowerProfiles (PPD shim)"
                );
            } else {
                eprintln!(
                    "optid: PPD shim disabled — conflicting daemon(s) active. \
                     Mask power-profiles-daemon.service and restart optid to enable."
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
    if args.allowlist {
        // WP-N4: load seeded baseline + runtime overrides and arm the gate.
        let al = allowlist::Allowlist::load();
        // Record the effective allowlist at startup so the audit trail shows
        // exactly what the gate will admit (default-deny for everything else).
        let mut summary = format!(
            "optid: WP-N4 hardware allowlist gate ENABLED (default-deny); \
             version={} effective_entries={}\n",
            al.version(),
            al.len()
        );
        for entry in al.entries() {
            summary.push_str("optid:   allowlist ");
            summary.push_str(&entry.describe());
            summary.push('\n');
        }
        append_log(&args.state_dir.join("decisions.log"), &summary)?;
        actuator.enable_allowlist(al);
    }

    loop {
        let override_mode = read_mode_override(&args.state_dir).unwrap_or(Mode::Auto);
        let mut snapshot = Snapshot::collect();
        snapshot.global_pinned_class = read_global_pinned_class(&args.state_dir);
        if let Some(ref app) = snapshot.foreground_app {
            snapshot.pinned_class = read_pinned_class(&args.state_dir, app);
        }

        let policy = Policy::load(&args.config_path);
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

        let decision = policy.decide_resolved(
            &snapshot,
            override_mode,
            committed_class,
            class_reason,
            &contracts,
            Some(resolved_mode),
            mode_hysteresis_reason,
        );
        let report = decision.render(&snapshot);

        fs::write(args.state_dir.join("status"), &report)?;
        append_log(&args.state_dir.join("decisions.log"), &report)?;

        if args.apply {
            for action in &decision.actions {
                actuator.apply(action)?;
            }
        }

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
