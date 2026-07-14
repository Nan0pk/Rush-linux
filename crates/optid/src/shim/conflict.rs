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

/// Detect conflicts using the default `systemctl is-active` checker. See
/// `detect_conflicts_with` for the injectable-checker variant used by tests.
pub(crate) fn detect_conflicts(daemons: &[String]) -> ConflictReport {
    detect_conflicts_with(daemons, systemd_is_active)
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
}
