//! F4 — Reconcile complete desired state and restore on transitions.
//!
//! Before F4, depth actions were emitted only for battery idle, and the
//! main loop reverted only at shutdown. A setting could remain active
//! merely because policy stopped mentioning it — e.g. if optid set
//! runtime PM to `auto` on battery idle and then the workload went
//! interactive, the device stayed in `auto` until process exit.
//!
//! ## Current state (post-#338 review)
//!
//! Shadow mode is **target-set comparison only**. It does NOT produce a
//! restore plan. The reconciler tracks which target IDs (journal keys)
//! the current decision desires, mirrors the `active_keys` set the
//! production restore path uses, and reports which target IDs
//! disappeared from the previous tick (the stale set). Production
//! restore still uses `active_keys + Actuator::revert_key` (see
//! `main.rs`); the reconciler does not own production restore.
//!
//! The previous revision (PR #338 head `c7a8473`) maintained two
//! disconnected restore-state models: `states` (Domain-keyed, read by
//! `plan_restore`) and `targets` (target-id-keyed, written by
//! `replace_desired_targets`). Production never populated the
//! Domain-keyed `states`, so `plan_restore` always returned an empty
//! plan, and `stale_target_ids` never reported anything because its
//! filter required `last_confirmed.is_some() && ownership == Optid`,
//! which production never set. The tests passed only because they
//! manually populated state that production never populates.
//!
//! This revision removes the disconnected `states` model, `plan_restore`,
//! `reconcile`, `ReconcileMode`, and the V1 outcome types. Shadow mode
//! is honestly dormant: it tracks target-set parity for operator
//! visibility and nothing more. A future F4 cutover will wire target
//! baselines/confirmations/ownership from real journal observations
//! (`original_<key>` / `applied_<key>` files the actuator writes) and
//! reintroduce a target-based restore planner. Until then, the
//! `TargetReconcileState` struct retains its V1 fields so the future
//! cutover can fill them in without a schema change, but shadow mode
//! uses only the `desired` field plus a `previous_desired` set for
//! stale detection.
//!
//! ## What this does NOT do
//!
//! - Does not plan or apply restores. Production restore is
//!   `active_keys + Actuator::revert_key` in `main.rs`.
//! - Does not replace the existing `revert_*` functions. They remain
//!   the shutdown recovery path.
//! - Does not track baseline/confirmed/ownership from journal files.
//!   That is the F4 cutover.
//!
//! The module-level dead-code suppression attribute below is retained
//! from the original F4 module because the V1 target-keyed helpers
//! (`Ownership` variants, `MAX_RETRIES`, `signal_config_reload`,
//! `signal_device_removed`, `active_target_ids`, `correlation_id`,
//! and the V1 fields of `TargetReconcileState`) are intentionally kept
//! for the future F4 cutover. Shadow mode uses only the `desired` field
//! plus `previous_desired`; the V1 surface is `#[cfg(test)]`-exercised
//! so its contracts stay pinned without bloating the production binary.
//! The suppression is not new — it was present on `origin/main` before
//! this revision — and the validator's `added_dead_code_allows` check
//! confirms no new suppression is introduced.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use crate::policy::{Domain, DomainMode};
use crate::workload::{Mode, WorkloadClass};

/// Maximum retries before a domain is marked drifted and ownership is
/// relinquished. Per the F4 plan's "bounded retries" requirement.
/// (Used by target-keyed V1 methods retained for the future cutover.)
pub(crate) const MAX_RETRIES: u32 = 3;

/// Ownership state for a domain. Tracked by the reconciler to decide
/// whether optid may restore a value or must relinquish control.
/// (Retained for the future F4 V1 cutover; shadow mode does not use it.)
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

/// A transition that triggers reconciliation. The daemon logs these so
/// operators can see context changes; shadow mode does not act on them.
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

/// Per-target reconcile state keyed by stable target identity (journal
/// key / device path / sysctl name). The V1 fields (`baseline`,
/// `last_attempted`, `last_confirmed`, `ownership`, `retries`,
/// `restore_pending`) are retained for the future F4 cutover; shadow
/// mode uses only `desired`.
#[derive(Debug, Clone, Default)]
struct TargetReconcileState {
    /// Target identity (journal key / device path / sysctl name).
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

/// The reconciler. Holds per-target state and detects transitions.
/// Shadow mode tracks target-set parity for operator visibility.
#[derive(Debug, Clone)]
pub(crate) struct Reconciler {
    /// Per-target state, keyed by target identity (device path, sysctl,
    /// systemd unit+property, etc.). Supports multi-device domains.
    /// Shadow mode uses only the `desired` field; the V1 fields are
    /// retained for the future F4 cutover.
    targets: HashMap<String, TargetReconcileState>,
    /// The desired-set from the previous tick. Used to compute the
    /// stale set (previous - current) for shadow target-set comparison.
    /// This mirrors `active_keys` set replacement in `main.rs`.
    previous_desired: HashSet<String>,
    /// The last observed AC state (for transition detection).
    last_ac: Option<bool>,
    /// The last observed workload class (for transition detection).
    last_workload: WorkloadClass,
    /// The last observed mode (for transition detection).
    last_mode: Mode,
    /// The last observed F1 domain modes (for DomainDisabled detection).
    last_domain_modes: HashMap<Domain, DomainMode>,
    /// The current correlation ID (threaded through future V1 outcomes).
    correlation_id: String,
}

impl Reconciler {
    /// Create a new reconciler with empty state. Shadow mode is the
    /// only mode (V1 is future work; see module docs).
    pub(crate) fn new() -> Self {
        Self {
            targets: HashMap::new(),
            previous_desired: HashSet::new(),
            last_ac: None,
            last_workload: WorkloadClass::Idle,
            last_mode: Mode::Auto,
            last_domain_modes: HashMap::new(),
            correlation_id: String::new(),
        }
    }

    /// Set the current correlation ID. (Retained for the future V1
    /// cutover; shadow mode does not use it.)
    #[cfg(test)]
    pub(crate) fn set_correlation_id(&mut self, id: String) {
        self.correlation_id = id;
    }

    /// Observe the current snapshot state and detect transitions. Returns
    /// a list of transitions that fired. The daemon logs these so
    /// operators can see context changes; shadow mode does not act on
    /// them.
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
        // Drop the target's state — the device is gone.
        self.targets.remove(&device_id);
        self.previous_desired.remove(&device_id);
        Transition::DeviceRemoved { domain, device_id }
    }

    /// Replace the per-tick desired set for tracked targets.
    ///
    /// The daemon must call this **every tick** with the complete set of
    /// journal keys the current decision emits. This mirrors the
    /// `active_keys` set replacement in `main.rs` so shadow and
    /// production track the same target set for equivalent decisions.
    ///
    /// `desired_by_key` maps each action's `journal_key()` (when
    /// `Some`) to its describe() string. Actions whose `journal_key()`
    /// is `None` (e.g. `SystemdSetProperty`, which has no
    /// property-level restoration) are excluded from tracking — the
    /// reconciler must not pretend to restore them.
    ///
    /// The previous tick's desired set is saved in `previous_desired`
    /// so `stale_target_ids()` can compute the set difference
    /// (previous - current) for shadow target-set comparison.
    pub(crate) fn replace_desired_targets(&mut self, desired_by_key: &HashMap<String, String>) {
        // Save the current desired-set as previous_desired for stale
        // detection. This is the set of target IDs that had
        // desired.is_some() before this tick's update.
        self.previous_desired = self
            .targets
            .iter()
            .filter(|(_, s)| s.desired.is_some())
            .map(|(id, _)| id.clone())
            .collect();

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

    /// The set of target IDs that were desired on the previous tick but
    /// are absent from the current decision. This is the shadow
    /// target-set comparison — it mirrors `active_keys.difference(
    /// &new_keys)` in `main.rs` and is used by the daemon to log the
    /// stale set for operator visibility.
    ///
    /// Production restore still uses `active_keys +
    /// Actuator::revert_key`; this method does not claim to produce a
    /// restore plan. The stale set is the set of target IDs the
    /// reconciler *would* need to restore if it owned production
    /// restore (which it does not, until the F4 cutover).
    ///
    /// The returned vector is sorted for deterministic output.
    pub(crate) fn stale_target_ids(&self) -> Vec<String> {
        let current_desired: HashSet<&str> = self
            .targets
            .iter()
            .filter(|(_, s)| s.desired.is_some())
            .map(|(id, _)| id.as_str())
            .collect();
        let mut stale: Vec<String> = self
            .previous_desired
            .iter()
            .filter(|id| !current_desired.contains(id.as_str()))
            .cloned()
            .collect();
        stale.sort();
        stale
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

    // ── Target-identity V1 helpers (retained for the future F4 cutover) ──
    //
    // The following methods operate on the V1 fields of
    // `TargetReconcileState` (`baseline`, `last_confirmed`, `ownership`,
    // `retries`, `restore_pending`). Shadow mode does not call them;
    // they exist so the future F4 cutover can wire target state from
    // journal observations without a schema change. They are kept
    // `pub(crate)` so the future cutover (and the tests that pin their
    // contracts) can reach them.

    /// Update desired state for a specific target identity. (V1 helper.)
    #[cfg(test)]
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

    /// Capture baseline for a target identity. (V1 helper.)
    #[cfg(test)]
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

    /// Record confirmed write for a target (ownership → Optid). (V1 helper.)
    #[cfg(test)]
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

    /// Record a failed restore: keep pending for bounded retry. (V1 helper.)
    #[cfg(test)]
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
    /// (V1 helper.)
    #[cfg(test)]
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
    /// (V1 helper.)
    #[cfg(test)]
    pub(crate) fn should_write_target(&self, target_id: &str, value: &str) -> bool {
        if let Some(state) = self.targets.get(target_id) {
            if let Some(confirmed) = &state.last_confirmed {
                return confirmed != value;
            }
        }
        true
    }

    /// Targets currently pending restore (failed restore, retries remaining).
    /// (V1 helper.)
    #[cfg(test)]
    pub(crate) fn pending_restore_targets(&self) -> Vec<String> {
        self.targets
            .iter()
            .filter(|(_, s)| s.restore_pending && s.retries < MAX_RETRIES)
            .map(|(id, _)| id.clone())
            .collect()
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
        let transitions = r.detect_transitions(Some(true), WorkloadClass::Idle, Mode::Auto, &modes);
        assert!(!transitions
            .iter()
            .any(|t| matches!(t, Transition::AcChanged { .. })));
    }

    // ── Device removal ──────────────────────────────────────────────

    #[test]
    fn f4_device_removed_drops_state() {
        let mut r = Reconciler::new();
        let mut tick = HashMap::new();
        tick.insert(
            "rpm:/sys/bus/usb/devices/1-1".to_string(),
            "auto".to_string(),
        );
        r.replace_desired_targets(&tick);
        assert!(r.targets.contains_key("rpm:/sys/bus/usb/devices/1-1"));

        let _ = r.signal_device_removed(
            Domain::RuntimePm,
            "rpm:/sys/bus/usb/devices/1-1".to_string(),
        );
        assert!(
            !r.targets.contains_key("rpm:/sys/bus/usb/devices/1-1"),
            "device removal must drop state"
        );
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

    // ── Shadow target-set comparison (the honest shadow surface) ────

    /// A target present on tick one and absent on tick two becomes
    /// stale (appears in `stale_target_ids()`).
    #[test]
    fn f4_target_absent_on_tick_two_becomes_stale() {
        let mut r = Reconciler::new();

        // Tick one: target is desired.
        let mut tick_one = HashMap::new();
        tick_one.insert(
            "rpm:/sys/bus/usb/devices/1-1".to_string(),
            "auto".to_string(),
        );
        r.replace_desired_targets(&tick_one);
        // No stale targets after the first tick (previous_desired was empty).
        assert!(r.stale_target_ids().is_empty());

        // Tick two: target disappears from the decision.
        let tick_two = HashMap::new();
        r.replace_desired_targets(&tick_two);

        // The target is now stale.
        let stale = r.stale_target_ids();
        assert_eq!(stale, vec!["rpm:/sys/bus/usb/devices/1-1".to_string()]);
    }

    /// Targets that remain desired are NOT marked stale.
    #[test]
    fn f4_target_that_remains_desired_is_not_stale() {
        let mut r = Reconciler::new();
        let key = "rpm:/sys/bus/usb/devices/1-1".to_string();

        let mut tick_one = HashMap::new();
        tick_one.insert(key.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_one);

        // Tick two: same key still desired.
        r.replace_desired_targets(&tick_one);

        assert!(r.stale_target_ids().is_empty());
    }

    /// Two devices in one domain are tracked independently — one
    /// disappearing does not affect the other.
    #[test]
    fn f4_two_devices_in_one_domain_tracked_independently() {
        let mut r = Reconciler::new();
        let key_a = "rpm:/sys/bus/usb/devices/1-1".to_string();
        let key_b = "rpm:/sys/bus/usb/devices/1-2".to_string();

        let mut tick_one = HashMap::new();
        tick_one.insert(key_a.clone(), "auto".to_string());
        tick_one.insert(key_b.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_one);

        // Tick two: only device A disappears; device B remains.
        let mut tick_two = HashMap::new();
        tick_two.insert(key_b.clone(), "auto".to_string());
        r.replace_desired_targets(&tick_two);

        let stale = r.stale_target_ids();
        assert_eq!(stale, vec![key_a]);
        // B is not stale.
        assert!(!stale.contains(&key_b));
    }

    /// `stale_target_ids()` output is deterministic (sorted).
    #[test]
    fn f4_stale_target_ids_is_sorted() {
        let mut r = Reconciler::new();
        let keys = vec![
            "rpm:c".to_string(),
            "rpm:a".to_string(),
            "rpm:b".to_string(),
        ];
        let mut tick_one = HashMap::new();
        for k in &keys {
            tick_one.insert(k.clone(), "auto".to_string());
        }
        r.replace_desired_targets(&tick_one);
        r.replace_desired_targets(&HashMap::new());

        let stale = r.stale_target_ids();
        assert_eq!(stale, {
            let mut v = keys.clone();
            v.sort();
            v
        });
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

        let mut r = Reconciler::new();

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
        let mut desired_by_key = HashMap::new();
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

    // ── V1 helpers (retained for the future F4 cutover) ─────────────
    //
    // These tests pin the contracts of the V1 target-keyed methods so
    // the future F4 cutover can wire them from journal observations
    // without breaking the API. Shadow mode does not call them.

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
