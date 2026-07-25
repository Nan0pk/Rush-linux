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

/// Per-target reconcile state keyed by stable target identity (not only
/// broad domain). Supports multiple devices in one domain.
#[derive(Debug, Clone, Default)]
struct TargetReconcileState {
    /// Target identity (journal key / device path / sysctl name / systemd unit+property).
    target_id: String,
    /// Domain this target belongs to (when known).
    domain: Option<Domain>,
    baseline: Option<String>,
    desired: Option<String>,
    last_attempted: Option<String>,
    last_confirmed: Option<String>,
    ownership: Ownership,
    retries: u32,
    /// Pending restore after a failed attempt (bounded by MAX_RETRIES).
    restore_pending: bool,
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

/// The reconciler. Holds per-domain and per-target state and generates
/// restore actions on transitions.
#[derive(Debug, Clone)]
pub(crate) struct Reconciler {
    /// Per-domain state, keyed by `Domain` (legacy path; kept for parity).
    states: HashMap<Domain, DomainReconcileState>,
    /// Per-target state, keyed by target identity (device path, sysctl,
    /// systemd unit+property, etc.). Supports multi-device domains.
    targets: HashMap<String, TargetReconcileState>,
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
            targets: HashMap::new(),
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

    /// Update desired state for a specific target identity (multi-device
    /// domains, systemd properties, vm/sysctl knobs). Shadow and v1 share
    /// this path so parity tests can compare against active_keys.
    pub(crate) fn set_desired_target(&mut self, target_id: &str, value: Option<String>) {
        let state = self
            .targets
            .entry(target_id.to_string())
            .or_insert_with(|| TargetReconcileState {
                target_id: target_id.to_string(),
                ..Default::default()
            });
        if state.desired != value {
            state.desired = value;
            state.retries = 0;
        }
    }

    /// Capture baseline for a target identity.
    pub(crate) fn capture_baseline_target(&mut self, target_id: &str, value: String) {
        let state = self
            .targets
            .entry(target_id.to_string())
            .or_insert_with(|| TargetReconcileState {
                target_id: target_id.to_string(),
                ..Default::default()
            });
        state.baseline = Some(value);
    }

    /// Record confirmed write for a target (ownership → Optid).
    pub(crate) fn record_confirmed_target(&mut self, target_id: &str, confirmed_value: String) {
        let state = self
            .targets
            .entry(target_id.to_string())
            .or_insert_with(|| TargetReconcileState {
                target_id: target_id.to_string(),
                ..Default::default()
            });
        state.last_confirmed = Some(confirmed_value);
        state.retries = 0;
        state.restore_pending = false;
        if state.ownership == Ownership::Unowned {
            state.ownership = Ownership::Optid;
        }
    }

    /// Record a failed restore: keep pending for bounded retry.
    pub(crate) fn record_restore_failure_target(
        &mut self,
        target_id: &str,
        error: String,
    ) -> Ownership {
        let state = self
            .targets
            .entry(target_id.to_string())
            .or_insert_with(|| TargetReconcileState {
                target_id: target_id.to_string(),
                ..Default::default()
            });
        state.retries += 1;
        state.restore_pending = true;
        if state.retries >= MAX_RETRIES {
            state.ownership = Ownership::External {
                reason: format!("restore failed {} times: {}", state.retries, error),
            };
            state.restore_pending = false;
        } else if state.ownership == Ownership::Unowned {
            state.ownership = Ownership::Optid;
        }
        state.ownership.clone()
    }

    /// Evaluate whether optid may restore `target_id` to `baseline`.
    ///
    /// Restore only when readback still equals optid's last confirmed value.
    /// If another manager changed the value, relinquish ownership and report drift.
    pub(crate) fn may_restore_target(
        &mut self,
        target_id: &str,
        readback: &str,
    ) -> Result<String, Ownership> {
        let state = match self.targets.get_mut(target_id) {
            Some(s) => s,
            None => {
                return Err(Ownership::Unowned);
            }
        };
        if state.ownership != Ownership::Optid {
            return Err(state.ownership.clone());
        }
        match (&state.last_confirmed, &state.baseline) {
            (Some(confirmed), Some(baseline)) => {
                if readback == confirmed.as_str() {
                    Ok(baseline.clone())
                } else {
                    state.ownership = Ownership::External {
                        reason: format!("drift: readback {readback} != last_confirmed {confirmed}"),
                    };
                    Err(state.ownership.clone())
                }
            }
            _ => Err(Ownership::Unowned),
        }
    }

    /// Coalesce: skip write when desired already equals last confirmed.
    pub(crate) fn should_write_target(&self, target_id: &str, value: &str) -> bool {
        if let Some(state) = self.targets.get(target_id) {
            if let Some(confirmed) = &state.last_confirmed {
                return confirmed != value;
            }
        }
        true
    }

    /// Targets currently pending restore (failed restore, retries remaining).
    pub(crate) fn pending_restore_targets(&self) -> Vec<String> {
        self.targets
            .iter()
            .filter(|(_, s)| s.restore_pending && s.retries < MAX_RETRIES)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Shadow/production parity: set of target ids with non-None desired.
    pub(crate) fn active_target_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .targets
            .iter()
            .filter(|(_, s)| s.desired.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
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

    /// Compute the restore plan for a transition **without applying it**.
    ///
    /// This is the planning half of the shadow/v1 separation (post-#337
    /// repair). The plan is identical in `Shadow` and `V1` modes — only
    /// `reconcile()` (the apply path) differs: `Shadow` discards the
    /// plan and performs no writes; `V1` may later apply it.
    ///
    /// For each domain whose desired value changed or whose domain was
    /// disabled, the plan contains a restore action that reverts to the
    /// baseline. The plan is sorted by `Domain::as_str()` so output
    /// order is deterministic across runs and across modes.
    ///
    /// Production restore still uses `active_keys + Actuator::revert_key`
    /// (see `main.rs`); the reconciler does not own production restore
    /// until the F4 cutover. The daemon logs the shadow plan size so
    /// operators can see what the reconciler *would* restore.
    pub(crate) fn plan_restore(&self, transition: &Transition) -> Vec<ReconcileAction> {
        let mut actions = Vec::new();

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
                let mut stale: Vec<(Domain, &DomainReconcileState)> = self
                    .states
                    .iter()
                    .filter(|(_, s)| {
                        s.desired.is_none()
                            && s.last_confirmed.is_some()
                            && s.ownership == Ownership::Optid
                    })
                    .map(|(d, s)| (*d, s))
                    .collect();
                // Deterministic ordering: sort by domain's canonical key.
                stale.sort_by_key(|(d, _)| d.as_str());
                for (domain, state) in stale {
                    if let Some(baseline) = &state.baseline {
                        actions.push(ReconcileAction {
                            domain,
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
            Transition::DeviceRemoved { domain, .. } => {
                // State already dropped in signal_device_removed.
                // No restore action — the device is gone.
                let _ = domain;
            }
        }

        actions
    }

    /// Apply a restore plan in `V1` mode. In `Shadow` mode this is a
    /// no-op (the caller logs the plan via `plan_restore` but no writes
    /// are emitted). Production restore still uses `active_keys +
    /// Actuator::revert_key` until the F4 cutover; this method is the
    /// eventual V1 apply path and is not yet wired into the daemon.
    pub(crate) fn reconcile(&mut self, transition: &Transition) -> Vec<ReconcileAction> {
        if self.mode == ReconcilerMode::Shadow {
            // Shadow mode: log the plan but perform no writes. The plan
            // is still computed via `plan_restore` so the caller can
            // report the would-restore count.
            return Vec::new();
        }
        // V1 mode: apply the plan. The V1 apply path is not yet wired
        // into the daemon (F4 cutover is a separate change); the plan
        // is returned so a future caller can apply it.
        self.plan_restore(transition)
    }

    /// Replace the per-tick desired set for tracked targets.
    ///
    /// The daemon must call this **every tick** with the complete set of
    /// journal keys the current decision emits. Targets that were
    /// present on the previous tick but are absent from the current
    /// decision become `desired = None` (stale / restore-eligible).
    /// Targets that remain desired are updated to the new value. This
    /// mirrors the `active_keys` set replacement in `main.rs` so shadow
    /// and production track the same target set for equivalent
    /// decisions.
    ///
    /// `desired_by_key` maps each action's `journal_key()` (when
    /// `Some`) to its describe() string. Actions whose `journal_key()`
    /// is `None` (e.g. `SystemdSetProperty`, which has no
    /// property-level restoration) are excluded from tracking — the
    /// reconciler must not pretend to restore them.
    pub(crate) fn replace_desired_targets(
        &mut self,
        desired_by_key: &std::collections::HashMap<String, String>,
    ) {
        // Step 1: mark every currently-tracked target whose key is not
        // in the new desired set as `desired = None`. This makes the
        // target stale / restore-eligible on the next transition.
        for (target_id, state) in &mut self.targets {
            if !desired_by_key.contains_key(target_id) && state.desired.is_some() {
                state.desired = None;
                state.retries = 0;
            }
        }
        // Step 2: insert or update desired for every key in the new
        // set. Reset retries when the desired value changes.
        for (key, describe) in desired_by_key {
            let state = self
                .targets
                .entry(key.clone())
                .or_insert_with(|| TargetReconcileState {
                    target_id: key.clone(),
                    ..Default::default()
                });
            if state.desired.as_deref() != Some(describe.as_str()) {
                state.desired = Some(describe.clone());
                state.retries = 0;
            }
        }
    }

    /// Snapshot of the current target IDs that are stale (desired =
    /// None) and were previously confirmed (last_confirmed is Some).
    /// Used by the daemon to log the would-restore set in shadow mode
    /// without coupling to internal state. The returned vector is
    /// sorted for deterministic output.
    pub(crate) fn stale_target_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .targets
            .iter()
            .filter(|(_, s)| {
                s.desired.is_none() && s.last_confirmed.is_some() && s.ownership == Ownership::Optid
            })
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
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
        // Planning is mode-independent: shadow computes the same plan
        // as v1 so the daemon can log what *would* be restored.
        let plan = r.plan_restore(&transition);
        assert_eq!(
            plan.len(),
            1,
            "shadow plan_restore must compute the same non-empty plan as v1"
        );
        // Apply path returns empty in shadow — no writes are emitted.
        let actions = r.reconcile(&transition);
        assert!(actions.is_empty(), "shadow mode must not emit writes");
    }

    // ── Post-#337: shadow planning truthfulness ────────────────────────

    /// Shadow and V1 must compute the same restore plan for the same
    /// state + transition. This is the core parity invariant: shadow
    /// logs the plan; v1 (when wired in) applies it. If the plans
    /// diverge, shadow is lying about what v1 would do.
    #[test]
    fn f4_shadow_and_v1_compute_same_plan() {
        let make_reconciler = |mode| {
            let mut r = Reconciler::new().with_mode(mode);
            r.capture_baseline(Domain::RuntimePm, "on".to_string());
            r.set_desired(Domain::RuntimePm, Some("auto".to_string()));
            r.record_confirmed(Domain::RuntimePm, "auto".to_string());
            r.set_desired(Domain::RuntimePm, None);
            r
        };
        let transition = Transition::AcChanged {
            from: Some(false),
            to: Some(true),
        };
        let shadow_plan = make_reconciler(ReconcilerMode::Shadow).plan_restore(&transition);
        let v1_plan = make_reconciler(ReconcilerMode::V1).plan_restore(&transition);
        assert_eq!(shadow_plan, v1_plan);
        assert_eq!(shadow_plan.len(), 1);
        assert_eq!(shadow_plan[0].domain, Domain::RuntimePm);
        assert_eq!(shadow_plan[0].value, "on");
    }

    /// A target present on tick one and absent on tick two becomes
    /// stale (desired = None, last_confirmed = Some). The next
    /// transition's plan_restore must include it.
    #[test]
    fn f4_target_absent_on_tick_two_becomes_stale() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);

        // Tick one: target is desired and confirmed.
        let mut tick_one = std::collections::HashMap::new();
        tick_one.insert(
            "rpm:/sys/bus/usb/devices/1-1".to_string(),
            "auto".to_string(),
        );
        r.replace_desired_targets(&tick_one);
        r.record_confirmed_target("rpm:/sys/bus/usb/devices/1-1", "auto".to_string());
        r.capture_baseline_target("rpm:/sys/bus/usb/devices/1-1", "on".to_string());

        // Tick two: target disappears from the decision.
        let tick_two = std::collections::HashMap::new();
        r.replace_desired_targets(&tick_two);

        // The target is now stale.
        let stale = r.stale_target_ids();
        assert_eq!(stale, vec!["rpm:/sys/bus/usb/devices/1-1".to_string()]);
    }

    /// Targets that remain desired are NOT marked stale.
    #[test]
    fn f4_target_that_remains_desired_is_not_stale() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);
        let key = "rpm:/sys/bus/usb/devices/1-1".to_string();

        let mut tick_one = std::collections::HashMap::new();
        tick_one.insert(key.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_one);
        r.record_confirmed_target(&key, "auto".to_string());

        // Tick two: same key still desired.
        r.replace_desired_targets(&tick_one);

        assert!(r.stale_target_ids().is_empty());
    }

    /// Two devices in one domain are tracked independently — restoring
    /// one must not affect the other.
    #[test]
    fn f4_two_devices_in_one_domain_tracked_independently() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);
        let key_a = "rpm:/sys/bus/usb/devices/1-1".to_string();
        let key_b = "rpm:/sys/bus/usb/devices/1-2".to_string();

        let mut tick_one = std::collections::HashMap::new();
        tick_one.insert(key_a.clone(), "auto".to_string());
        tick_one.insert(key_b.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_one);
        r.record_confirmed_target(&key_a, "auto".to_string());
        r.record_confirmed_target(&key_b, "auto".to_string());
        r.capture_baseline_target(&key_a, "on".to_string());
        r.capture_baseline_target(&key_b, "on".to_string());

        // Tick two: only device A disappears; device B remains.
        let mut tick_two = std::collections::HashMap::new();
        tick_two.insert(key_b.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_two);

        let stale = r.stale_target_ids();
        assert_eq!(stale, vec![key_a]);
        // B is not stale.
        assert!(!stale.contains(&key_b));
    }

    /// Drifted/external targets are excluded from the stale set: optid
    /// no longer owns them, so the reconciler must not pretend to
    /// restore them.
    #[test]
    fn f4_drifted_targets_excluded_from_stale() {
        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);
        let key = "rpm:/sys/bus/usb/devices/1-1".to_string();

        let mut tick_one = std::collections::HashMap::new();
        tick_one.insert(key.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_one);
        r.record_confirmed_target(&key, "auto".to_string());
        r.capture_baseline_target(&key, "on".to_string());

        // An external manager changes the value; optid relinquishes.
        let _ = r.may_restore_target(&key, "on").unwrap_err();

        // Tick two: target disappears from the decision.
        r.replace_desired_targets(&std::collections::HashMap::new());

        // The target is drifted (External), not stale/restore-eligible.
        let stale = r.stale_target_ids();
        assert!(
            stale.is_empty(),
            "drifted target must not be stale: {stale:?}"
        );
    }

    /// `plan_restore` output order is deterministic across runs (sorted
    /// by `Domain::as_str()`).
    #[test]
    fn f4_plan_restore_order_is_deterministic() {
        let make_reconciler = || {
            let mut r = Reconciler::new().with_mode(ReconcilerMode::V1);
            // Insert in non-canonical order: Backlight, CpuEpp, RuntimePm.
            for (domain, baseline) in [
                (Domain::Backlight, "80".to_string()),
                (Domain::CpuEpp, "performance".to_string()),
                (Domain::RuntimePm, "auto".to_string()),
            ] {
                r.capture_baseline(domain, baseline);
                r.set_desired(domain, Some("x".to_string()));
                r.record_confirmed(domain, "x".to_string());
                r.set_desired(domain, None);
            }
            r
        };
        let transition = Transition::ConfigReloaded;
        let plan_a = make_reconciler().plan_restore(&transition);
        let plan_b = make_reconciler().plan_restore(&transition);
        assert_eq!(plan_a, plan_b);
        // Domains appear in canonical (as_str) sorted order.
        let domains: Vec<&str> = plan_a.iter().map(|a| a.domain.as_str()).collect();
        let mut sorted = domains.clone();
        sorted.sort();
        assert_eq!(domains, sorted);
    }

    /// Shadow target IDs match the current `active_keys` set for
    /// equivalent decisions. The daemon builds `active_keys` from
    /// `Action::journal_key()`; `replace_desired_targets` builds the
    /// reconciler's target set from the same keys. The two sets must
    /// agree so shadow and production track the same surface.
    #[test]
    fn f4_shadow_target_ids_match_active_keys() {
        use crate::action::Action;
        use std::collections::HashSet;
        use std::path::PathBuf;

        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);

        // Build a decision's action set the same way the daemon does.
        let actions = vec![
            Action::RuntimePm {
                device_dir: PathBuf::from("/sys/bus/usb/devices/1-1"),
                autosuspend_delay_ms: 2000,
                reason: "test".to_string(),
            },
            Action::CpuEpp {
                value: "power".to_string(),
                reason: "test".to_string(),
            },
            Action::SystemdSetProperty {
                unit: "user.slice".to_string(),
                properties: vec!["CPUWeight=100".to_string()],
                reason: "test".to_string(),
            },
        ];

        // active_keys: the production set the daemon builds.
        let active_keys: HashSet<String> = actions.iter().filter_map(|a| a.journal_key()).collect();

        // desired_by_key: the same map the daemon passes to
        // replace_desired_targets.
        let mut desired_by_key = std::collections::HashMap::new();
        for action in &actions {
            if let Some(key) = action.journal_key() {
                desired_by_key.insert(key, action.describe());
            }
        }
        r.replace_desired_targets(&desired_by_key);

        // The reconciler's active target IDs must equal active_keys.
        let shadow_active: HashSet<String> = r.active_target_ids().into_iter().collect();
        assert_eq!(
            shadow_active, active_keys,
            "shadow target IDs must match active_keys for equivalent decisions"
        );

        // SystemdSetProperty's journal_key is None, so it must NOT
        // appear in either set — the reconciler must not pretend to
        // restore an action with no property-level restoration.
        assert!(
            !shadow_active.iter().any(|k| k.starts_with("systemd_")),
            "systemd keys must not be tracked (no property-level restoration)"
        );
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

    // ── Target-identity tracking (multi-device, drift, coalesce) ────

    #[test]
    fn f4_target_tracks_multiple_devices_per_domain() {
        let mut r = Reconciler::new();
        r.set_desired_target("rpm:/sys/devices/pci0/00:1.0", Some("auto".into()));
        r.set_desired_target("rpm:/sys/devices/pci0/00:2.0", Some("auto".into()));
        r.record_confirmed_target("rpm:/sys/devices/pci0/00:1.0", "auto".into());
        r.record_confirmed_target("rpm:/sys/devices/pci0/00:2.0", "auto".into());
        let active = r.active_target_ids();
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|id| id.contains("00:1.0")));
        assert!(active.iter().any(|id| id.contains("00:2.0")));
    }

    #[test]
    fn f4_target_restore_only_if_last_confirmed() {
        let mut r = Reconciler::new();
        r.capture_baseline_target("rpm:dev1", "on".into());
        r.record_confirmed_target("rpm:dev1", "auto".into());
        // Readback still matches last confirmed → restore to baseline allowed.
        let baseline = r.may_restore_target("rpm:dev1", "auto").unwrap();
        assert_eq!(baseline, "on");
    }

    #[test]
    fn f4_target_drift_relinquishes() {
        let mut r = Reconciler::new();
        r.capture_baseline_target("rpm:dev1", "on".into());
        r.record_confirmed_target("rpm:dev1", "auto".into());
        // External manager changed value to "on" without optid.
        let err = r.may_restore_target("rpm:dev1", "on").unwrap_err();
        assert!(matches!(err, Ownership::External { .. }));
    }

    #[test]
    fn f4_target_coalesce_identical_writes() {
        let mut r = Reconciler::new();
        r.record_confirmed_target("sysctl:vm.swappiness", "100".into());
        assert!(!r.should_write_target("sysctl:vm.swappiness", "100"));
        assert!(r.should_write_target("sysctl:vm.swappiness", "60"));
    }

    #[test]
    fn f4_target_failed_restore_stays_pending_bounded() {
        let mut r = Reconciler::new();
        r.capture_baseline_target("rpm:dev1", "on".into());
        r.record_confirmed_target("rpm:dev1", "auto".into());
        r.record_restore_failure_target("rpm:dev1", "EBUSY".into());
        assert_eq!(r.pending_restore_targets().len(), 1);
        for _ in 0..MAX_RETRIES {
            r.record_restore_failure_target("rpm:dev1", "EBUSY".into());
        }
        // After MAX_RETRIES, no longer pending; ownership External.
        assert!(r.pending_restore_targets().is_empty());
        let st = r.targets.get("rpm:dev1").unwrap();
        assert!(matches!(st.ownership, Ownership::External { .. }));
    }

    #[test]
    fn f4_shadow_active_targets_parity_with_key_set() {
        // Shadow tracks the same journal keys active_keys would.
        let mut r = Reconciler::new().with_mode(ReconcilerMode::Shadow);
        let keys = ["rpm:a", "rpm:b", "sysctl:vm.swappiness"];
        for k in keys {
            r.set_desired_target(k, Some("v".into()));
        }
        let active = r.active_target_ids();
        assert_eq!(active, {
            let mut v = keys.to_vec();
            v.sort();
            v.into_iter().map(str::to_string).collect::<Vec<_>>()
        });
        // Clearing desired mirrors active_keys removal.
        r.set_desired_target("rpm:a", None);
        assert!(!r.active_target_ids().iter().any(|id| id == "rpm:a"));
    }

    #[test]
    fn f4_target_supports_systemd_and_sysctl_identities() {
        let mut r = Reconciler::new();
        r.set_desired_target("systemd:user@1000.service:CPUWeight", Some("100".into()));
        r.set_desired_target("sysctl:vm.dirty_bytes", Some("134217728".into()));
        r.capture_baseline_target("systemd:user@1000.service:CPUWeight", "100".into());
        r.record_confirmed_target("systemd:user@1000.service:CPUWeight", "200".into());
        r.capture_baseline_target("sysctl:vm.dirty_bytes", "0".into());
        r.record_confirmed_target("sysctl:vm.dirty_bytes", "134217728".into());
        assert_eq!(r.active_target_ids().len(), 2);
        // Drift on systemd property.
        let err = r
            .may_restore_target("systemd:user@1000.service:CPUWeight", "50")
            .unwrap_err();
        assert!(matches!(err, Ownership::External { .. }));
    }
}
