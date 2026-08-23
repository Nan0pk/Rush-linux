//! v0.6 Phase B3 — conflict detection for competing policy daemons.
//!
//! `optid` is the single owner of hardware policy on a Rush Linux host (ADR
//! 0004). If `tlp.service`, `tuned.service`, or `power-profiles-daemon.service`
//! is already running, `optid --apply` would fight them for `/sys/firmware/
//! acpi/platform_profile`, runtime-PM state, and EPP — exactly the
//! inconsistent-write scenario the WP-N4 allowlist is meant to prevent.
//!
//! This module checks systemd for active conflicting services at startup. If
//! any are active and `--apply` was requested, the caller MUST downgrade to
//! dry-run and log the conflict. The check itself is non-fatal: a missing
//! `systemctl` binary or an unreachable systemd D-Bus is treated as "no
//! conflicts detected" so the daemon can still start in containers and
//! non-systemd environments.
//!
//! The systemd-active check is abstracted behind a `SystemdChecker` function
//! pointer so unit tests can inject a mock without spawning subprocesses. The
//! production path (`detect_conflicts`) shells out to `systemctl is-active`,
//! matching the existing pattern in `actuator.rs::Action::SystemdSetProperty`.
//!
//! See `docs/plans/v0.6-hardware-aware-optid-proposal.md` §3 Phase B (B3).

use std::process::Command;

/// A function that returns `true` if the named systemd service is currently
/// active. The default implementation shells out to `systemctl is-active`;
/// tests inject a mock.
pub(crate) type SystemdChecker = fn(&str) -> bool;

/// Report returned by `detect_conflicts`. Carries the list of active
/// conflicting daemons (in the order they appear in the input list) and
/// exposes helpers for the caller's logging decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConflictReport {
    pub active_conflicts: Vec<String>,
}

impl ConflictReport {
    /// `true` if at least one conflicting daemon is active. The caller should
    /// refuse `--apply` and stay in dry-run.
    pub fn is_blocking(&self) -> bool {
        !self.active_conflicts.is_empty()
    }

    /// Human-readable advice for the operator, suitable for `eprintln!` and
    /// the decisions log. Suggests the exact `systemctl mask --now` command
    /// so the operator can copy-paste.
    pub fn render_advice(&self) -> String {
        if self.active_conflicts.is_empty() {
            return String::new();
        }
        let list = self.active_conflicts.join(" ");
        format!(
            "Conflicting policy daemons are active: {list}. \
             optid is the single owner of hardware policy (ADR 0004); \
             running alongside these daemons will produce inconsistent \
             kernel writes. Either stop optid or mask the conflicting \
             daemons: `systemctl mask --now {list}`. \
             optid will stay in dry-run until the conflict is resolved."
        )
    }
}

/// Default systemd-active check. Shells out to `systemctl is-active <name>`
/// and returns `true` if stdout is exactly `active\n`. Any error (binary
/// missing, non-zero exit, non-`active` output) returns `false` — the
/// conflict check fails open, not closed, so the daemon can still start
/// in containers and non-systemd environments.
///
/// This function is `fn` (not a closure) so it can be stored as a
/// `SystemdChecker` function pointer without boxing.
pub(crate) fn systemd_is_active(service: &str) -> bool {
    let Ok(output) = Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .output()
    else {
        return false;
    };
    output.status.success()
}

/// Detect conflicts using the default `systemctl is-active` checker, unless a
/// test on this thread has installed an override (see
/// `with_conflict_checker_override`). See `detect_conflicts_with` for the
/// injectable-checker variant used directly by unit tests in this module.
pub(crate) fn detect_conflicts(daemons: &[String]) -> ConflictReport {
    #[cfg(test)]
    {
        let overridden = CONFLICT_CHECKER_OVERRIDE.with(|slot| *slot.borrow());
        if let Some(checker) = overridden {
            return detect_conflicts_with(daemons, checker);
        }
    }
    detect_conflicts_with(daemons, systemd_is_active)
}

// `crate::run()` calls `detect_conflicts` directly rather than going through
// an injected `KernelIo`: this check spawns `systemctl is-active`, a process
// execution, not the file I/O `KernelIo` models, so it needs its own seam.
// End-to-end production-surface tests (e.g. S2D's) need `detect_conflicts` to
// answer deterministically regardless of which policy daemon happens to be
// active on the host running the suite — mirrors
// `kernel_io::with_real_kernel_override`.
#[cfg(test)]
thread_local! {
    static CONFLICT_CHECKER_OVERRIDE: std::cell::RefCell<Option<SystemdChecker>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct ConflictCheckerGuard(Option<SystemdChecker>);

#[cfg(test)]
impl Drop for ConflictCheckerGuard {
    fn drop(&mut self) {
        CONFLICT_CHECKER_OVERRIDE.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

/// Run a binary-crate test with `detect_conflicts`'s systemd check on the
/// current thread routed through `checker` instead of a real `systemctl`
/// call. The previous override is restored even if the test unwinds.
#[cfg(test)]
pub(crate) fn with_conflict_checker_override<R>(
    checker: SystemdChecker,
    run: impl FnOnce() -> R,
) -> R {
    let previous = CONFLICT_CHECKER_OVERRIDE.with(|slot| slot.replace(Some(checker)));
    let _guard = ConflictCheckerGuard(previous);
    run()
}

/// Detect conflicts using a caller-supplied checker. Exposed for tests so
/// they don't have to spawn `systemctl`. Production callers should use
/// `detect_conflicts`.
pub(crate) fn detect_conflicts_with(daemons: &[String], checker: SystemdChecker) -> ConflictReport {
    let active_conflicts = daemons
        .iter()
        .filter(|d| checker(d))
        .cloned()
        .collect::<Vec<_>>();
    ConflictReport { active_conflicts }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker_active_for(active: &[&str]) -> SystemdChecker {
        // We can't close over `active` in a fn pointer, so we use a static
        // thread-local for test scope. This is test-only code; the production
        // path uses the real `systemd_is_active` directly.
        // Alternative: refactor to a trait + generic. Done as fn-pointer to
        // match the proposal's "smallest surface" intent.
        thread_local! {
            static ACTIVE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        ACTIVE.with(|a| {
            *a.borrow_mut() = active.iter().map(|s| s.to_string()).collect();
        });
        fn check(svc: &str) -> bool {
            ACTIVE.with(|a| a.borrow().iter().any(|s| s == svc))
        }
        check
    }

    #[test]
    fn no_conflicts_when_no_daemons_active() {
        let daemons = vec![
            "tlp.service".to_string(),
            "tuned.service".to_string(),
            "power-profiles-daemon.service".to_string(),
        ];
        let checker = checker_active_for(&[]);
        let report = detect_conflicts_with(&daemons, checker);
        assert!(!report.is_blocking());
        assert!(report.active_conflicts.is_empty());
        assert_eq!(report.render_advice(), "");
    }

    #[test]
    fn detects_single_active_conflict() {
        let daemons = vec![
            "tlp.service".to_string(),
            "tuned.service".to_string(),
            "power-profiles-daemon.service".to_string(),
        ];
        let checker = checker_active_for(&["tlp.service"]);
        let report = detect_conflicts_with(&daemons, checker);
        assert!(report.is_blocking());
        assert_eq!(report.active_conflicts, vec!["tlp.service".to_string()]);
        let advice = report.render_advice();
        assert!(advice.contains("tlp.service"), "{advice}");
        assert!(
            advice.contains("systemctl mask --now tlp.service"),
            "{advice}"
        );
    }

    #[test]
    fn detects_multiple_active_conflicts_in_input_order() {
        let daemons = vec![
            "tlp.service".to_string(),
            "tuned.service".to_string(),
            "power-profiles-daemon.service".to_string(),
        ];
        // Multiple conflicts — order should match the input order, not the
        // checker's iteration order.
        let checker = checker_active_for(&["power-profiles-daemon.service", "tlp.service"]);
        let report = detect_conflicts_with(&daemons, checker);
        assert!(report.is_blocking());
        assert_eq!(
            report.active_conflicts,
            vec![
                "tlp.service".to_string(),
                "power-profiles-daemon.service".to_string()
            ]
        );
        let advice = report.render_advice();
        // Both services appear in the mask command.
        assert!(advice.contains("tlp.service"), "{advice}");
        assert!(advice.contains("power-profiles-daemon.service"), "{advice}");
    }

    #[test]
    fn empty_daemon_list_is_never_blocking() {
        let checker = checker_active_for(&["tlp.service"]);
        let report = detect_conflicts_with(&[], checker);
        assert!(!report.is_blocking());
        assert!(report.active_conflicts.is_empty());
    }

    #[test]
    fn unknown_daemons_in_list_are_ignored() {
        // Daemons not in the input list don't appear in the report even if
        // the checker says they're active.
        let daemons = vec!["tlp.service".to_string()];
        let checker = checker_active_for(&["tuned.service", "tlp.service"]);
        let report = detect_conflicts_with(&daemons, checker);
        assert_eq!(report.active_conflicts, vec!["tlp.service".to_string()]);
    }

    #[test]
    fn render_advice_is_empty_when_no_conflicts() {
        let report = ConflictReport {
            active_conflicts: vec![],
        };
        assert_eq!(report.render_advice(), "");
    }

    #[test]
    fn render_advice_mentions_adr_0004_and_mask_command() {
        let report = ConflictReport {
            active_conflicts: vec!["tlp.service".to_string()],
        };
        let advice = report.render_advice();
        assert!(
            advice.contains("ADR 0004"),
            "advice should reference ADR 0004: {advice}"
        );
        assert!(advice.contains("systemctl mask --now"), "{advice}");
        assert!(advice.contains("dry-run"), "{advice}");
    }

    #[test]
    fn detect_conflicts_production_entrypoint_honors_the_override() {
        let daemons = vec!["tuned.service".to_string()];
        let with_no_conflicts =
            with_conflict_checker_override(|_service| false, || detect_conflicts(&daemons));
        assert!(!with_no_conflicts.is_blocking());

        let with_a_conflict =
            with_conflict_checker_override(|_service| true, || detect_conflicts(&daemons));
        assert!(with_a_conflict.is_blocking());
    }

    #[test]
    fn conflict_checker_override_is_restored_after_the_closure_returns() {
        let daemons = vec!["tuned.service".to_string()];
        with_conflict_checker_override(
            |_service| true,
            || {
                assert!(detect_conflicts(&daemons).is_blocking());
            },
        );
        // No override installed here: falls through to the real
        // `systemctl is-active` check, which cannot report a conflict for a
        // service name no unit file on this or any host defines.
        let unregistered = vec!["optid-conflict-test-sentinel.service".to_string()];
        assert!(!detect_conflicts(&unregistered).is_blocking());
    }

    #[test]
    fn conflict_checker_override_survives_a_panicking_closure() {
        let result = std::panic::catch_unwind(|| {
            with_conflict_checker_override(
                |_service| true,
                || {
                    panic!("simulated test failure inside the override scope");
                },
            )
        });
        assert!(result.is_err());
        let daemons = vec!["optid-conflict-test-sentinel.service".to_string()];
        assert!(!detect_conflicts(&daemons).is_blocking());
    }
}
