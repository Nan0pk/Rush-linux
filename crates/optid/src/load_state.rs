//! Explicit load-state tracking for `optid`'s two config sources (policy.toml
//! and the hardware allowlist).
//!
//! ## Why this exists
//!
//! Before the optid-safety phase, `Policy::load` and `Allowlist::load` both
//! silently fell back to a default on any failure (missing file, unparseable
//! TOML, missing override directory). The fallback was "safe" in the sense
//! that the daemon did not crash, but it was **opaque**: the run loop had no
//! way to know whether the loaded configuration was the operator's intent or
//! a fallback. That meant a corrupt `policy.toml` could silently flip `optid`
//! from a tuned configuration to the default configuration, and the operator
//! would only notice by reading stderr.
//!
//! The optid-safety phase (audit #1, Critical) requires `optid` to fail
//! closed: a malformed policy must not silently enable dynamic writes. This
//! module provides the explicit `LoadState` enum that the run loop checks
//! before arming the actuator.
//!
//! ## State semantics
//!
//! - `Ok` — the file was present and parsed cleanly. Every section the
//!   operator intended is loaded. Dynamic writes are permitted (subject to
//!   `--apply` and the allowlist gate).
//! - `Defaulted` — the file was missing. `optid` fell back to the curated
//!   baseline policy (or the seeded allowlist baseline). This is the
//!   expected state on a fresh install before the operator writes a config.
//!   The curated baseline is permitted; dynamic writes are not.
//! - `Partial` — the file was present and parseable as TOML, but one or more
//!   optional sections were missing or had wrong types. `optid` filled in
//!   the missing sections from the curated baseline. The curated baseline is
//!   permitted; dynamic writes are not (the operator's intent is ambiguous).
//! - `Invalid` — the file was present but unparseable (TOML syntax error) or
//!   failed a structural validation. `optid` fell back to the curated
//!   baseline. The curated baseline is permitted; dynamic writes are not.
//!
//! ## What "dynamic writes" means
//!
//! "Dynamic writes" are the per-cycle `Action`s produced by
//! `Policy::decide_resolved` — `CpuEpp`, `PlatformProfile`, `VmSysctl`,
//! `CpuDmaLatency`, `DeviceResumeLatency`, `RuntimePm`, `PcieAspm`,
//! `SataAlpm`, `Backlight`, `SystemdSetProperty`. These are the writes that
//! change as the workload changes.
//!
//! "Curated baseline writes" are a small, fixed, conservative set of writes
//! applied once at startup (and reverted on shutdown). They are the safety
//! floor: even when the policy is malformed, `optid` still puts the system
//! into a known-good state. See `Policy::curated_baseline`.
//!
//! ## What "baseline armed" means
//!
//! `baseline_armed` is `true` whenever the daemon is running with `--apply`
//! (i.e., not in dry-run). It is independent of the load states: the curated
//! baseline is safe by construction, so it is applied even when the policy is
//! `Invalid`. The only condition that disarms the baseline is dry-run mode,
//! because dry-run mode means "do not write anything at all".
//!
//! ## What "apply armed" means
//!
//! `apply_armed` is `true` only when ALL of the following hold:
//!
//! 1. `--apply` was passed (not dry-run).
//! 2. `policy_load_state == Ok`.
//! 3. `allowlist_load_state == Ok` (or the allowlist gate is disabled via
//!    `--no-allowlist`, in which case the allowlist load state is not
//!    consulted).
//! 4. No competing daemons were detected (the conflict check did not
//!    downgrade `--apply` to dry-run).
//!
//! If any condition fails, `apply_armed` is `false` and the actuator skips
//! every dynamic `Action` with a logged reason. The curated baseline is
//! still applied (subject to `baseline_armed`).

use std::fmt;

/// The load state of a single config source (`policy.toml` or the hardware
/// allowlist). See the module docstring for the full semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadState {
    /// File present, parsed cleanly. Dynamic writes permitted.
    Ok,
    /// File missing. Curated baseline used. Dynamic writes disabled.
    Defaulted,
    /// File present, parseable, but missing optional sections. Curated
    /// baseline filled in the gaps. Dynamic writes disabled.
    Partial,
    /// File present but unparseable or structurally invalid. Curated
    /// baseline used. Dynamic writes disabled.
    Invalid,
}

impl LoadState {
    /// `true` when the source loaded cleanly enough to permit dynamic writes.
    pub(crate) fn permits_dynamic_writes(self) -> bool {
        matches!(self, LoadState::Ok)
    }

    /// `true` when the source is in any of the fallback states. The curated
    /// baseline is permitted in all of these; dynamic writes are not.
    #[allow(dead_code)]
    pub(crate) fn is_fallback(self) -> bool {
        matches!(
            self,
            LoadState::Defaulted | LoadState::Partial | LoadState::Invalid
        )
    }

    /// A short machine-readable label suitable for log lines.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LoadState::Ok => "ok",
            LoadState::Defaulted => "defaulted",
            LoadState::Partial => "partial",
            LoadState::Invalid => "invalid",
        }
    }
}

impl fmt::Display for LoadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The decision surface for one cycle: which load states were observed, and
/// which write classes are armed as a result. The allowlist half is decided
/// once at startup; the policy half is re-decided every cycle, because the run
/// loop re-reads `policy.toml` on each pass and a reload that stops returning
/// `Ok` must disarm dynamic writes for as long as it keeps failing.
///
/// A single value is immutable — the actuator reads it and never mutates it —
/// but the run loop hands the actuator a fresh one per cycle, so a stale
/// `policy_load_state` can never keep writes armed after the configuration
/// behind them became unreadable.
///
/// This used to say `policy.toml` was read only at startup. It is not, and
/// believing it was is what let a failed reload fall back to
/// `Policy::curated_baseline()` — whose per-domain default is `actuate` —
/// while `apply_armed` still said `true` from startup.
#[derive(Debug, Clone)]
pub(crate) struct BootState {
    /// The load state of `config/optid/policy.toml`.
    pub(crate) policy_load_state: LoadState,
    /// The load state of the hardware allowlist (seeded baseline + override
    /// dirs). `LoadState::Ok` when the allowlist gate is disabled via
    /// `--no-allowlist` (no load is attempted).
    pub(crate) allowlist_load_state: LoadState,
    /// `true` when dynamic per-cycle `Action`s may be applied. See the
    /// module docstring for the full condition list.
    pub(crate) apply_armed: bool,
    /// `true` when the curated baseline may be applied at startup (and
    /// reverted on shutdown). Independent of the load states; disarmed only
    /// by dry-run mode.
    pub(crate) baseline_armed: bool,
    /// `true` when the allowlist gate is enabled (i.e., `--allowlist` was
    /// not turned off). Recorded here so the actuator knows whether to
    /// consult `allowlist_load_state` for the `apply_armed` decision.
    pub(crate) allowlist_gate_enabled: bool,
}

impl BootState {
    /// A human-readable one-line summary for the startup log.
    pub(crate) fn summary(&self) -> String {
        format!(
            "policy_load_state={} allowlist_load_state={} allowlist_gate={} apply_armed={} baseline_armed={}",
            self.policy_load_state,
            self.allowlist_load_state,
            self.allowlist_gate_enabled,
            self.apply_armed,
            self.baseline_armed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_dynamic_writes_only_when_ok() {
        assert!(LoadState::Ok.permits_dynamic_writes());
        assert!(!LoadState::Defaulted.permits_dynamic_writes());
        assert!(!LoadState::Partial.permits_dynamic_writes());
        assert!(!LoadState::Invalid.permits_dynamic_writes());
    }

    #[test]
    fn is_fallback_for_non_ok_states() {
        assert!(!LoadState::Ok.is_fallback());
        assert!(LoadState::Defaulted.is_fallback());
        assert!(LoadState::Partial.is_fallback());
        assert!(LoadState::Invalid.is_fallback());
    }

    #[test]
    fn as_str_is_stable() {
        // Pin the labels — they appear in log files and must not drift.
        assert_eq!(LoadState::Ok.as_str(), "ok");
        assert_eq!(LoadState::Defaulted.as_str(), "defaulted");
        assert_eq!(LoadState::Partial.as_str(), "partial");
        assert_eq!(LoadState::Invalid.as_str(), "invalid");
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", LoadState::Ok), "ok");
        assert_eq!(format!("{}", LoadState::Defaulted), "defaulted");
        assert_eq!(format!("{}", LoadState::Partial), "partial");
        assert_eq!(format!("{}", LoadState::Invalid), "invalid");
    }

    #[test]
    fn boot_state_summary_lists_all_fields() {
        let bs = BootState {
            policy_load_state: LoadState::Ok,
            allowlist_load_state: LoadState::Ok,
            apply_armed: true,
            baseline_armed: true,
            allowlist_gate_enabled: true,
        };
        let s = bs.summary();
        assert!(s.contains("policy_load_state=ok"), "summary: {s}");
        assert!(s.contains("allowlist_load_state=ok"), "summary: {s}");
        assert!(s.contains("allowlist_gate=true"), "summary: {s}");
        assert!(s.contains("apply_armed=true"), "summary: {s}");
        assert!(s.contains("baseline_armed=true"), "summary: {s}");
    }
}
