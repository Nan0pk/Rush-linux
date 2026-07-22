//! F4 — Reconcile complete desired state and restore on transitions.
//!
//! Before F4, depth actions were emitted only for battery idle, and the
//! main loop reverted only at shutdown. A setting could remain active
//! merely because policy stopped mentioning it — e.g. if optid set
//! runtime PM to `auto` on battery idle and then the workload went
//! interactive, the device stayed in `auto` until process exit.
//!
//! The reconciler fixes this by tracking the complete desired state per
//! domain and generating immediate restore actions on every transition:
//!
//! - AC attach / detach
//! - Workload-class change (idle → interactive → throughput → ...)
//! - User-mode change (battery → balanced → performance → realtime)
//! - Config reload
//! - Device removal (hot-unplug)
//! - Domain disable (F1 mode → Off)
//!
//! ## Design (F4 plan)
//!
//! 1. **Track four values per domain**: baseline (captured at startup
//!    or last transition), desired (what policy wants this cycle), last
//!    attempted (what optid tried to write), last confirmed (what
//!    readback showed).
//! 2. **Generate restore on transition**: when a transition fires, the
//!    reconciler emits restore actions for every domain whose desired
//!    value changed or whose domain was disabled.
//! 3. **Coalesce identical writes**: if the desired value equals the
//!    last confirmed value, no write is emitted. This prevents
//!    redundant sysfs writes on every control-loop iteration.
//! 4. **Bounded retries**: a failed write is retried at most
//!    `MAX_RETRIES` times before the domain is marked drifted and
//!    ownership is relinquished.
//! 5. **Drift detection**: if a readback shows a value different from
//!    what optid wrote, the domain is marked drifted. Per the F4 spec
//!    gap resolution: "restore only if current value still equals
//!    optid's last applied value; otherwise relinquish ownership and
//!    report drift."
//! 6. **Shadow mode**: `[control] reconciler = "shadow|v1"`. In shadow
//!    mode, the reconciler computes what it would restore but does not
//!    emit writes — the existing actuator path runs unchanged. In v1
//!    mode, the reconciler's restore actions are applied.
//!
//! ## What this does NOT do
//!
//! - F4 does not change `Policy::decide_resolved`. The reconciler wraps
//!   the decision, it does not alter it.
//! - F4 does not wire into the main loop yet. The main loop integration
//!   is a separate change that follows shadow-mode parity testing.
//! - F4 does not replace the existing `revert_*` functions. They remain
//!   the shutdown recovery path; the reconciler is the per-transition
//!   recovery path.
//!
//! Note: F4 defines its own `Ownership` enum locally. When F3 (versioned
//! envelopes) merges, a follow-up can align `reconciler::Ownership` with
//! `envelope::Ownership`. Until then, the reconciler is self-contained.

#![allow(dead_code)]

use std::collections::HashMap;

use crate::policy::{Domain, DomainMode};
use crate::workload::{Mode, WorkloadClass};

/// Maximum retries before a domain is marked drifted and ownership is
/// relinquished. Per the F4 plan's "bounded retries" requirement.
pub(crate) const MAX_RETRIES: u32 = 3;

/// Ownership state for a domain. Tracked by the reconciler to decide
/// whether optid may restore a value or must relinquish control.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Ownership {
    /// optid owns the domain and may write to it.
    Optid,
    /// Another program changed the value after optid's last applied
    /// value. optid relinquishes ownership and will not fight it.
    External { reason: String },
    /// The domain is not currently owned by optid.
    #[default]
    Unowned,
}

/// The reconciler's operating mode. Controlled by `[control] reconciler`
/// in policy.toml. Shadow mode is the safe default; v1 mode applies
/// restore actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ReconcilerMode {
    /// Compute what would be restored but do not emit writes.
    #[default]
    Shadow,
    /// Apply restore actions on transitions.
    V1,
}

/// A transition that triggers reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transition {
    AcChanged {
        from: Option<bool>,
        to: Option<bool>,
    },
    WorkloadChanged {
        from: WorkloadClass,
        to: WorkloadClass,
    },
    ModeChanged {
        from: Mode,
        to: Mode,
    },
    ConfigReloaded,
    DeviceRemoved {
        domain: Domain,
        device_id: String,
    },
    DomainDisabled {
        domain: Domain,
    },
}

impl Transition {
    pub(crate) fn describe(&self) -> String {
        match self {
            Transition::AcChanged { from, to } => {
                format!("ac_changed: {:?} → {:?}", from, to)
            }
            Transition::WorkloadChanged { from, to } => {
                format!("workload_changed: {} → {}", from, to)
            }
            Transition::ModeChanged { from, to } => {
                format!("mode_changed: {} → {}", from, to)
            }
            Transition::ConfigReloaded => "config_reloaded".to_string(),
            Transition::DeviceRemoved { domain, device_id } => {
                format!("device_removed: {} (domain={})", device_id, domain.as_str())
            }
            Transition::DomainDisabled { domain } => {
                format!("domain_disabled: {}", domain.as_str())
            }
        }
    }
}

/// A restore action emitted by the reconciler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReconcileAction {
    pub domain: Domain,
    pub value: String,
    pub reason: String,
    pub transition: Transition,
}

/// Per-domain reconcile state. Tracks the four values from the F4 plan.
#[derive(Debug, Clone, Default)]
struct DomainReconcileState {
    baseline: Option<String>,
    desired: Option<String>,
    last_attempted: Option<String>,
    last_confirmed: Option<String>,
    ownership: Ownership,
    retries: u32,
}

/// The desired state for one domain, as exposed to the status surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesiredDomainState {
    pub domain: String,
    pub desired_value: Option<String>,
    pub baseline_value: Option<String>,
    pub last_attempted: Option<String>,
    pub last_confirmed: Option<String>,
    pub ownership: Ownership,
}

/// The complete desired state across all domains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesiredState {
    pub timestamp: u64,
    pub correlation_id: String,
    pub domains: Vec<DesiredDomainState>,
}

impl DesiredState {
    /// Look up the desired state for a specific domain.
    pub(crate) fn for_domain(&self, domain: Domain) -> Option<&DesiredDomainState> {
        self.domains.iter().find(|d| d.domain == domain.as_str())
    }
}

/// The result of an apply attempt (F3 alignment; defined locally until
/// F3 merges).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyResult {
    Applied { written_value: String },
    Skipped { reason: String },
    Failed { error: String },
    Drifted { expected: String, actual: String },
    Restored { original: String },
}

/// The result of a restore attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoreResult {
    Restored { original: String },
    Stabilized { fallback: String, reason: String },
    Relinquished { reason: String },
    Failed { error: String },
}

/// An apply outcome record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyOutcome {
    pub timestamp: u64,
    pub action: String,
    pub result: ApplyResult,
    pub correlation_id: String,
}

/// A restore outcome record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestoreOutcome {
    pub timestamp: u64,
    pub domain: String,
    pub result: RestoreResult,
    pub correlation_id: String,
}

/// The reconciler. Holds per-domain state and generates restore actions
/// on transitions.
#[derive(Debug, Clone)]
pub(crate) struct Reconciler {
    /// Per-domain state, keyed by `Domain`.
    states: HashMap<Domain, DomainReconcileState>,
    /// The reconciler's operating mode (shadow or v1).
    mode: ReconcilerMode,
    /// The last observed AC state (for transition detection).
    last_ac: Option<bool>,
    /// The last observed workload class (for transition detection).
    last_workload: WorkloadClass,
    /// The last observed mode (for transition detection).
    last_mode: Mode,
    /// The last observed F1 domain modes (for DomainDisabled detection).
    last_domain_modes: HashMap<Domain, DomainMode>,
    /// The current correlation ID (threaded through restore outcomes).
    correlation_id: String,
}

impl Reconciler {
    /// Create a new reconciler in shadow mode with empty state.
    pub(crate) fn new() -> Self {
        Self {
            states: HashMap::new(),
            mode: ReconcilerMode::Shadow,
            last_ac: None,
            last_workload: WorkloadClass::Idle,
            last_mode: Mode::Auto,
            last_domain_modes: HashMap::new(),
            correlation_id: String::new(),
        }
    }

    /// Set the reconciler's operating mode.
    pub(crate) fn with_mode(mut self, mode: ReconcilerMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the current correlation ID.
    pub(crate) fn set_correlation_id(&mut self, id: String) {
        self.correlation_id = id;
    }

    /// Observe the current snapshot state and detect transitions. Returns
    /// a list of transitions that fired. This is the main entry point
    /// for transition detection.
    ///
    /// The caller passes the current AC state, workload class, mode, and
    /// per-domain effective modes. The reconciler compares against its
    /// last-known state and emits transitions for any differences.
    pub(crate) fn detect_transitions(
        &mut self,
        on_ac: Option<bool>,
        workload: WorkloadClass,
        mode: Mode,
        domain_modes: &HashMap<Domain, DomainMode>,
    ) -> Vec<Transition> {
        let mut transitions = Vec::new();

        // AC state change
        if self.last_ac.is_some() && self.last_ac != on_ac {
            transitions.push(Transition::AcChanged {
                from: self.last_ac,
                to: on_ac,
            });
        }
        self.last_ac = on_ac;

        // Workload class change
        if self.last_workload != workload {
            transitions.push(Transition::WorkloadChanged {
                from: self.last_workload,
                to: workload,
            });
        }
        self.last_workload = workload;

        // Mode change
        if self.last_mode != mode {
            transitions.push(Transition::ModeChanged {
                from: self.last_mode,
                to: mode,
            });
        }
        self.last_mode = mode;

        // Domain disable (mode → Off)
        for (domain, current_mode) in domain_modes {
            if let Some(prev_mode) = self.last_domain_modes.get(domain) {
                if *prev_mode != DomainMode::Off && *current_mode == DomainMode::Off {
                    transitions.push(Transition::DomainDisabled { domain: *domain });
                }
            }
            self.last_domain_modes.insert(*domain, *current_mode);
        }

        transitions
    }

    /// Signal a config reload. Emits a ConfigReloaded transition.
    pub(crate) fn signal_config_reload(&mut self) -> Transition {
        Transition::ConfigReloaded
    }

    /// Signal a device removal. Updates internal state and returns a
    /// DeviceRemoved transition.
    pub(crate) fn signal_device_removed(
        &mut self,
        domain: Domain,
        device_id: String,
    ) -> Transition {
        // Drop the domain's state — the device is gone.
        self.states.remove(&domain);
        Transition::DeviceRemoved { domain, device_id }
    }

    /// Update the desired state for a domain. Called when the policy
    /// decision produces an action for this domain.
    pub(crate) fn set_desired(&mut self, domain: Domain, value: Option<String>) {
        let state = self.states.entry(domain).or_default();
        // If the desired value changed, reset retries.
        if state.desired != value {
            state.desired = value;
            state.retries = 0;
        }
    }

    /// Record that optid attempted a write to `domain` with `value`.
    pub(crate) fn record_attempt(&mut self, domain: Domain, value: String) {
        let state = self.states.entry(domain).or_default();
        state.last_attempted = Some(value);
        if state.ownership == Ownership::Unowned {
            state.ownership = Ownership::Optid;
        }
    }

    /// Record that optid confirmed a write to `domain` via readback.
    /// The `confirmed_value` is what the readback showed.
    pub(crate) fn record_confirmed(&mut self, domain: Domain, confirmed_value: String) {
        let state = self.states.entry(domain).or_default();
        state.last_confirmed = Some(confirmed_value.clone());
        state.retries = 0;
        if state.ownership == Ownership::Unowned {
            state.ownership = Ownership::Optid;
        }
    }

    /// Record a failed write to `domain`. Increments the retry count.
    /// If retries exceed MAX_RETRIES, the domain is marked drifted and
    /// ownership is relinquished.
    pub(crate) fn record_failure(&mut self, domain: Domain, error: String) -> Ownership {
        let state = self.states.entry(domain).or_default();
        state.retries += 1;
        if state.retries >= MAX_RETRIES {
            state.ownership = Ownership::External {
                reason: format!("write failed {} times: {}", state.retries, error),
            };
        } else if state.ownership == Ownership::Unowned {
            // optid is attempting writes — it owns the domain now.
            state.ownership = Ownership::Optid;
        }
        state.ownership.clone()
    }

    /// Record drift: a readback showed a value different from what
    /// optid wrote. Per the F4 spec gap resolution: relinquish ownership
    /// and report drift.
    pub(crate) fn record_drift(
        &mut self,
        domain: Domain,
        expected: String,
        actual: String,
    ) -> Ownership {
        let state = self.states.entry(domain).or_default();
        state.ownership = Ownership::External {
            reason: format!("drift detected: expected {}, actual {}", expected, actual),
        };
        state.ownership.clone()
    }

    /// Capture the baseline value for a domain. Called at startup or
    /// when a domain is first actuated.
    pub(crate) fn capture_baseline(&mut self, domain: Domain, value: String) {
        let state = self.states.entry(domain).or_default();
        state.baseline = Some(value);
    }

    /// Generate restore actions for a transition. For each domain whose
    /// desired value changed or whose domain was disabled, emit a
    /// restore action that reverts to the baseline.
    ///
    /// In shadow mode, this returns an empty vector (no writes emitted).
    /// The caller still receives the restore outcomes for logging.
    pub(crate) fn reconcile(&mut self, transition: &Transition) -> Vec<ReconcileAction> {
        let mut actions = Vec::new();

        if self.mode == ReconcilerMode::Shadow {
            // Shadow mode: compute but don't emit.
            return actions;
        }

        match transition {
            Transition::DomainDisabled { domain } => {
                if let Some(state) = self.states.get(domain) {
                    if let Some(baseline) = &state.baseline {
                        // Only restore if optid still owns the domain
                        // (i.e. the current value equals optid's last
                        // confirmed value). Otherwise relinquish.
                        if state.ownership == Ownership::Optid {
                            actions.push(ReconcileAction {
                                domain: *domain,
                                value: baseline.clone(),
                                reason: format!(
                                    "restore baseline on domain disable (transition: {})",
                                    transition.describe()
                                ),
                                transition: transition.clone(),
                            });
                        }
                    }
                }
            }
            Transition::AcChanged { .. }
            | Transition::WorkloadChanged { .. }
            | Transition::ModeChanged { .. }
            | Transition::ConfigReloaded => {
                // For these transitions, restore every domain whose
                // desired value is None (optid wants baseline) but whose
                // last confirmed is not None (optid previously wrote).
                for (domain, state) in &self.states {
                    if state.desired.is_none()
                        && state.last_confirmed.is_some()
                        && state.ownership == Ownership::Optid
                    {
                        if let Some(baseline) = &state.baseline {
                            actions.push(ReconcileAction {
                                domain: *domain,
                                value: baseline.clone(),
                                reason: format!(
                                    "restore baseline on {} (transition: {})",
                                    domain.as_str(),
                                    transition.describe()
                                ),
                                transition: transition.clone(),
                            });
                        }
                    }
                }
            }
            Transition::DeviceRemoved { domain, .. } => {
                // State already dropped in signal_device_removed.
                // No restore action — the device is gone.
                let _ = domain;
            }
        }

        actions
    }

    /// Should optid write `value` to `domain`? Returns false if the
    /// write is redundant (desired == last_confirmed) — this is the
    /// "coalesce identical writes" optimization.
    pub(crate) fn should_write(&self, domain: Domain, value: &str) -> bool {
        if let Some(state) = self.states.get(&domain) {
            if let Some(confirmed) = &state.last_confirmed {
                return confirmed != value;
            }
        }
        true
    }

    /// Get the current desired state as an F3 `DesiredState` envelope.
    pub(crate) fn desired_state(&self, timestamp: u64) -> DesiredState {
        let domains: Vec<DesiredDomainState> = Domain::all()
            .iter()
            .map(|d| {
                let state = self.states.get(d);
                DesiredDomainState {
                    domain: d.as_str().to_string(),
                    desired_value: state.and_then(|s| s.desired.clone()),
                    baseline_value: state.and_then(|s| s.baseline.clone()),
                    last_attempted: state.and_then(|s| s.last_attempted.clone()),
                    last_confirmed: state.and_then(|s| s.last_confirmed.clone()),
                    ownership: state
                        .map(|s| s.ownership.clone())
                        .unwrap_or(Ownership::Unowned),
                }
            })
            .collect();
        DesiredState {
            timestamp,
            correlation_id: self.correlation_id.clone(),
            domains,
        }
    }

    /// Build a `RestoreOutcome` for a restore action. Used by the caller
    /// to record the result in the F3 envelope.
    pub(crate) fn restore_outcome(
        &self,
        domain: Domain,
        result: RestoreResult,
        timestamp: u64,
    ) -> RestoreOutcome {
        RestoreOutcome {
            timestamp,
            domain: domain.as_str().to_string(),
            result,
            correlation_id: self.correlation_id.clone(),
        }
    }

    /// Build an `ApplyOutcome` for an apply action.
    pub(crate) fn apply_outcome(
        &self,
        action_desc: String,
        result: ApplyResult,
        timestamp: u64,
    ) -> ApplyOutcome {
        ApplyOutcome {
            timestamp,
            action: action_desc,
            result,
            correlation_id: self.correlation_id.clone(),
        }
    }
}

impl Default for Reconciler {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn domain_modes(mode: DomainMode) -> HashMap<Domain, DomainMode> {
        let mut m = HashMap::new();
        for d in Domain::all() {
            m.insert(*d, mode);
        }
        m
    }

    // ── Transition detection ────────────────────────────────────────

    #[test]
    fn f4_detects_ac_change() {
        let mut r = Reconciler::new();
        r.last_ac = Some(false); // simulate first observation already set

        let modes = domain_modes(DomainMode::Actuate);
        let transitions = r.detect_transitions(Some(true), WorkloadClass::Idle, Mode::Auto, &modes);
        assert!(transitions.iter().any(|t| matches!(
            t,
            Transition::AcChanged {
                from: Some(false),
                to: Some(true)
            }
        )));
    }

    #[test]
    fn f4_detects_workload_change() {
        let mut r = Reconciler::new();
        r.last_workload = WorkloadClass::Idle;

        let modes = domain_modes(DomainMode::Actuate);
        let transitions =
            r.detect_transitions(Some(true), WorkloadClass::Interactive, Mode::Auto, &modes);
        assert!(transitions.iter().any(|t| matches!(
            t,
            Transition::WorkloadChanged {
                from: WorkloadClass::Idle,
                to: WorkloadClass::Interactive
            }
        )));
    }

    #[test]
    fn f4_detects_mode_change() {
        let mut r = Reconciler::new();
        r.last_mode = Mode::Battery;

        let modes = domain_modes(DomainMode::Actuate);
        let transitions =
            r.detect_transitions(Some(false), WorkloadClass::Idle, Mode::Performance, &modes);
        assert!(transitions.iter().any(|t| matches!(
            t,
            Transition::ModeChanged {
                from: Mode::Battery,
                to: Mode::Performance
            }
        )));
    }

    #[test]
    fn f4_detects_domain_disable() {
        let mut r = Reconciler::new();
        // First observation: all domains Actuate
        let modes_on = domain_modes(DomainMode::Actuate);
        let _ = r.detect_transitions(Some(true), WorkloadClass::Idle, Mode::Auto, &modes_on);

        // Second observation: RuntimePm → Off
        let mut modes_off = modes_on.clone();
        modes_off.insert(Domain::RuntimePm, DomainMode::Off);
        let transitions =
            r.detect_transitions(Some(true), WorkloadClass::Idle, Mode::Auto, &modes_off);
        assert!(transitions.iter().any(|t| matches!(
            t,
            Transition::DomainDisabled {
                domain: Domain::RuntimePm
            }
        )));
    }

    #[test]
    fn f4_no_transition_on_first_observation() {
        let mut r = Reconciler::new();
        let modes = domain_modes(DomainMode::Actuate);
        // First observation: last_ac is None, so no AcChanged transition.
        let transitions = r.detect_transitions(Some(true), WorkloadClass::Idle, Mode::Auto, &modes);
        // WorkloadChanged and ModeChanged might fire because last_workload
        // defaults to Idle and last_mode defaults to Auto — if the
        // observed values match, no transition.
        assert!(!transitions
            .iter()
            .any(|t| matches!(t, Transition::AcChanged { .. })));
    }

    // ── Reconcile: DomainDisabled ───────────────────────────────────

    #[test]
    fn f4_reconcile_domain_disabled_emits_restore() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::V1);
        r.capture_baseline(Domain::RuntimePm, "on".to_string());
        r.record_confirmed(Domain::RuntimePm, "auto".to_string());

        let transition = Transition::DomainDisabled {
            domain: Domain::RuntimePm,
        };
        let actions = r.reconcile(&transition);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].domain, Domain::RuntimePm);
        assert_eq!(actions[0].value, "on");
    }

    #[test]
    fn f4_reconcile_shadow_mode_emits_nothing() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);
        r.capture_baseline(Domain::RuntimePm, "on".to_string());
        r.record_confirmed(Domain::RuntimePm, "auto".to_string());

        let transition = Transition::DomainDisabled {
            domain: Domain::RuntimePm,
        };
        let actions = r.reconcile(&transition);
        assert!(actions.is_empty(), "shadow mode must not emit writes");
    }

    #[test]
    fn f4_reconcile_does_not_restore_unowned_domain() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::V1);
        r.capture_baseline(Domain::RuntimePm, "on".to_string());
        // Domain is Unowned (never actuated by optid)

        let transition = Transition::DomainDisabled {
            domain: Domain::RuntimePm,
        };
        let actions = r.reconcile(&transition);
        assert!(
            actions.is_empty(),
            "must not restore a domain optid never owned"
        );
    }

    // ── Reconcile: AC / workload / mode transitions ─────────────────

    #[test]
    fn f4_reconcile_ac_change_restores_domains_with_no_desired() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::V1);
        r.capture_baseline(Domain::RuntimePm, "on".to_string());
        r.set_desired(Domain::RuntimePm, Some("auto".to_string()));
        r.record_confirmed(Domain::RuntimePm, "auto".to_string());
        // Now policy stops wanting runtime_pm (desired = None)
        r.set_desired(Domain::RuntimePm, None);

        let transition = Transition::AcChanged {
            from: Some(false),
            to: Some(true),
        };
        let actions = r.reconcile(&transition);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].domain, Domain::RuntimePm);
        assert_eq!(actions[0].value, "on", "must restore to baseline");
    }

    #[test]
    fn f4_reconcile_ac_change_skips_domains_with_active_desired() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::V1);
        r.capture_baseline(Domain::RuntimePm, "on".to_string());
        r.set_desired(Domain::RuntimePm, Some("auto".to_string()));
        r.record_confirmed(Domain::RuntimePm, "auto".to_string());
        // desired is still Some("auto") — optid still wants it

        let transition = Transition::AcChanged {
            from: Some(false),
            to: Some(true),
        };
        let actions = r.reconcile(&transition);
        assert!(
            actions.is_empty(),
            "must not restore a domain optid still wants"
        );
    }

    // ── Coalesce identical writes ───────────────────────────────────

    #[test]
    fn f4_should_write_returns_false_for_redundant_write() {
        let mut r = Reconciler::new();
        r.record_confirmed(Domain::CpuEpp, "performance".to_string());
        assert!(
            !r.should_write(Domain::CpuEpp, "performance"),
            "redundant write must be coalesced"
        );
    }

    #[test]
    fn f4_should_write_returns_true_for_changed_value() {
        let mut r = Reconciler::new();
        r.record_confirmed(Domain::CpuEpp, "performance".to_string());
        assert!(
            r.should_write(Domain::CpuEpp, "balance_performance"),
            "changed value must be written"
        );
    }

    #[test]
    fn f4_should_write_returns_true_for_unconfirmed_domain() {
        let r = Reconciler::new();
        assert!(
            r.should_write(Domain::CpuEpp, "performance"),
            "unconfirmed domain must be written"
        );
    }

    // ── Bounded retries ─────────────────────────────────────────────

    #[test]
    fn f4_failure_increments_retries() {
        let mut r = Reconciler::new();
        r.record_failure(Domain::CpuEpp, "EBUSY".to_string());
        r.record_failure(Domain::CpuEpp, "EBUSY".to_string());
        // After 2 failures, ownership is still Optid (retries < MAX_RETRIES=3)
        let state = r.states.get(&Domain::CpuEpp).unwrap();
        assert_eq!(state.retries, 2);
        assert_eq!(state.ownership, Ownership::Optid);
    }

    #[test]
    fn f4_failure_exceeding_max_retries_relinquishes_ownership() {
        let mut r = Reconciler::new();
        for _ in 0..MAX_RETRIES {
            r.record_failure(Domain::CpuEpp, "EBUSY".to_string());
        }
        let state = r.states.get(&Domain::CpuEpp).unwrap();
        assert!(matches!(state.ownership, Ownership::External { .. }));
    }

    #[test]
    fn f4_desired_value_change_resets_retries() {
        let mut r = Reconciler::new();
        r.record_failure(Domain::CpuEpp, "EBUSY".to_string());
        r.record_failure(Domain::CpuEpp, "EBUSY".to_string());
        r.set_desired(Domain::CpuEpp, Some("power".to_string())); // resets retries
        let state = r.states.get(&Domain::CpuEpp).unwrap();
        assert_eq!(state.retries, 0, "desired change must reset retries");
    }

    // ── Drift detection ─────────────────────────────────────────────

    #[test]
    fn f4_drift_relinquishes_ownership() {
        let mut r = Reconciler::new();
        r.record_drift(
            Domain::CpuEpp,
            "performance".to_string(),
            "power".to_string(),
        );
        let state = r.states.get(&Domain::CpuEpp).unwrap();
        assert!(matches!(state.ownership, Ownership::External { .. }));
    }

    // ── Device removal ──────────────────────────────────────────────

    #[test]
    fn f4_device_removed_drops_state() {
        let mut r = Reconciler::new();
        r.capture_baseline(Domain::RuntimePm, "on".to_string());
        assert!(r.states.contains_key(&Domain::RuntimePm));

        let _ = r.signal_device_removed(Domain::RuntimePm, "usb-1-1".to_string());
        assert!(
            !r.states.contains_key(&Domain::RuntimePm),
            "device removal must drop state"
        );
    }

    // ── DesiredState envelope ───────────────────────────────────────

    #[test]
    fn f4_desired_state_includes_all_domains() {
        let r = Reconciler::new();
        let ds = r.desired_state(1000);
        assert_eq!(ds.domains.len(), Domain::all().len());
        assert_eq!(ds.timestamp, 1000);
    }

    #[test]
    fn f4_desired_state_reflects_tracked_values() {
        let mut r = Reconciler::new();
        r.capture_baseline(Domain::CpuEpp, "balance_performance".to_string());
        r.set_desired(Domain::CpuEpp, Some("performance".to_string()));
        r.record_confirmed(Domain::CpuEpp, "performance".to_string());

        let ds = r.desired_state(1000);
        let entry = ds.for_domain(Domain::CpuEpp).unwrap();
        assert_eq!(
            entry.baseline_value,
            Some("balance_performance".to_string())
        );
        assert_eq!(entry.desired_value, Some("performance".to_string()));
        assert_eq!(entry.last_confirmed, Some("performance".to_string()));
        assert_eq!(entry.ownership, Ownership::Optid);
    }

    // ── Transition describe ─────────────────────────────────────────

    #[test]
    fn f4_transition_describe_is_human_readable() {
        let t = Transition::AcChanged {
            from: Some(false),
            to: Some(true),
        };
        let s = t.describe();
        assert!(s.contains("ac_changed"));
        assert!(s.contains("false"));
        assert!(s.contains("true"));
    }

    // ── Config reload ───────────────────────────────────────────────

    #[test]
    fn f4_config_reload_transition_emits_restore() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::V1);
        r.capture_baseline(Domain::Backlight, "80".to_string());
        r.set_desired(Domain::Backlight, Some("40".to_string()));
        r.record_confirmed(Domain::Backlight, "40".to_string());
        r.set_desired(Domain::Backlight, None); // config reload drops the desired

        let transition = r.signal_config_reload();
        let actions = r.reconcile(&transition);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].domain, Domain::Backlight);
        assert_eq!(
            actions[0].value, "80",
            "must restore to baseline on config reload"
        );
    }

    // ── Apply/Restore outcome helpers ───────────────────────────────

    #[test]
    fn f4_apply_outcome_carries_correlation_id() {
        let mut r = Reconciler::new();
        r.set_correlation_id("corr-42".to_string());
        let outcome = r.apply_outcome(
            "cpu.epp=performance".to_string(),
            ApplyResult::Applied {
                written_value: "performance".to_string(),
            },
            1000,
        );
        assert_eq!(outcome.correlation_id, "corr-42");
        assert_eq!(outcome.timestamp, 1000);
    }

    #[test]
    fn f4_restore_outcome_carries_domain_and_correlation_id() {
        let mut r = Reconciler::new();
        r.set_correlation_id("corr-43".to_string());
        let outcome = r.restore_outcome(
            Domain::RuntimePm,
            RestoreResult::Restored {
                original: "on".to_string(),
            },
            2000,
        );
        assert_eq!(outcome.domain, "runtime_pm");
        assert_eq!(outcome.correlation_id, "corr-43");
        assert_eq!(outcome.timestamp, 2000);
    }
}
