//! F4 production desired-state reconciliation.
//!
//! One target-keyed state model owns transition restoration.  The model is
//! hydrated from the typed F4 state file and from the legacy
//! `original_<key>`/`applied_<key>` journals, captures baselines before writes,
//! claims ownership only after confirmed readback, and restores only when the
//! current value still equals the last value confirmed as written by optid.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::actuator::Actuator;
use crate::actuators::display;
use crate::envelope::{
    ActionOutcome, ErrorKindCode, GateEvaluation, GateReasonCode, GateStage, OutcomeReasonCode,
    OwnershipState, PipelineStage, ReadbackOutcome, ResponsibleSubsystem, RestoreOutcome,
    RestoreState, SupportState, TargetOutcome, WriteOutcome,
};
use crate::io_util::{atomic_write_state_file_with, clear_journal_with};
use crate::kernel_io::KernelIo;
use crate::policy::{Domain, DomainMode};
use crate::sensors::discover_cpu_epp_paths_with;
use crate::workload::{Mode, WorkloadClass};

const STATE_FILE: &str = "reconciliation-state.json";
const MODE_FILE: &str = "reconciler-mode";
pub(crate) const MAX_RESTORE_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconcileMode {
    Shadow,
    V1,
}

impl ReconcileMode {
    fn load(io: &dyn KernelIo, state_dir: &Path) -> Self {
        match io
            .read_to_string(&state_dir.join(MODE_FILE))
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("shadow") => Self::Shadow,
            _ => Self::V1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transition {
    AcChanged { from: Option<bool>, to: Option<bool> },
    WorkloadChanged { from: WorkloadClass, to: WorkloadClass },
    ModeChanged { from: Mode, to: Mode },
    ConfigReloaded,
    DeviceRemoved { domain: Domain, device_id: String },
    DomainDisabled { domain: Domain },
}

impl Transition {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::AcChanged { from, to } => format!("ac_changed: {from:?} -> {to:?}"),
            Self::WorkloadChanged { from, to } => format!("workload_changed: {from} -> {to}"),
            Self::ModeChanged { from, to } => format!("mode_changed: {from} -> {to}"),
            Self::ConfigReloaded => "config_reloaded".to_string(),
            Self::DeviceRemoved { domain, device_id } => {
                format!("device_removed: {device_id} (domain={})", domain.as_str())
            }
            Self::DomainDisabled { domain } => {
                format!("domain_disabled: {}", domain.as_str())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TargetKind {
    KernelValue { path: PathBuf },
    PmqosCpu,
    PmqosDevice { path: PathBuf },
    RuntimePm {
        control_path: PathBuf,
        delay_path: Option<PathBuf>,
    },
    SystemdProperty { unit: String, property: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredValue {
    Scalar { value: String },
    RuntimePm {
        control: String,
        delay: Option<String>,
    },
    Systemd {
        explicit: bool,
        value: String,
    },
}

impl StoredValue {
    fn public_value(&self) -> String {
        match self {
            Self::Scalar { value } => value.clone(),
            Self::RuntimePm { control, delay } => match delay {
                Some(delay) => format!("control={control};delay={delay}"),
                None => format!("control={control};delay=absent"),
            },
            Self::Systemd { explicit, value } => {
                format!("explicit={explicit};value={value}")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetState {
    target_id: String,
    domain: String,
    target: TargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_journal_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline: Option<StoredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    desired: Option<StoredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_attempted: Option<StoredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_confirmed: Option<StoredValue>,
    ownership: OwnershipState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ownership_reason: Option<String>,
    retries: u32,
    restore_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    schema_version: u32,
    targets: BTreeMap<String, TargetState>,
}

#[derive(Debug, Clone)]
struct DesiredTarget {
    target_id: String,
    domain: String,
    target: TargetKind,
    desired: StoredValue,
    legacy_journal_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RestorePlan {
    target_id: String,
    target: TargetKind,
    baseline: StoredValue,
    last_confirmed: StoredValue,
    legacy_journal_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SystemdPropertyState {
    explicit: bool,
    value: String,
}

pub(crate) trait SystemdIo {
    fn read_property(&self, unit: &str, property: &str) -> io::Result<SystemdPropertyState>;
    fn set_property(&self, unit: &str, property: &str, value: Option<&str>) -> io::Result<()>;
}

#[derive(Debug, Default)]
struct RealSystemd;

impl RealSystemd {
    fn output(args: &[&str]) -> io::Result<String> {
        let output = Command::new("systemctl").args(args).output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "systemctl exited with {}",
                output.status
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn runtime_property_is_explicit(unit: &str, property: &str) -> io::Result<bool> {
        let paths = Self::output(&["show", "--property=DropInPaths", "--value", unit])?;
        for raw in paths.split_whitespace() {
            let path = raw.trim_matches('"');
            if !path.starts_with("/run/systemd/system.control/") {
                continue;
            }
            let Ok(content) = fs::read_to_string(path) else {
                continue;
            };
            if content.lines().any(|line| {
                line.trim_start()
                    .strip_prefix(property)
                    .is_some_and(|suffix| suffix.starts_with('='))
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl SystemdIo for RealSystemd {
    fn read_property(&self, unit: &str, property: &str) -> io::Result<SystemdPropertyState> {
        let selector = format!("--property={property}");
        let value = Self::output(&["show", &selector, "--value", unit])?
            .trim()
            .to_string();
        Ok(SystemdPropertyState {
            explicit: Self::runtime_property_is_explicit(unit, property)?,
            value,
        })
    }

    fn set_property(&self, unit: &str, property: &str, value: Option<&str>) -> io::Result<()> {
        let assignment = format!("{property}={}", value.unwrap_or(""));
        let status = Command::new("systemctl")
            .args(["set-property", "--runtime", unit, &assignment])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("systemctl exited with {status}")))
        }
    }
}

pub(crate) struct Reconciler {
    state_dir: PathBuf,
    mode: ReconcileMode,
    targets: BTreeMap<String, TargetState>,
    previous_desired: BTreeSet<String>,
    last_ac: Option<bool>,
    last_workload: WorkloadClass,
    last_mode: Mode,
    last_domain_modes: HashMap<Domain, DomainMode>,
    systemd: Box<dyn SystemdIo>,
}

impl Reconciler {
    pub(crate) fn load(state_dir: PathBuf, io: &dyn KernelIo) -> io::Result<Self> {
        Self::load_with_systemd(state_dir, io, Box::<RealSystemd>::default())
    }

    fn load_with_systemd(
        state_dir: PathBuf,
        io: &dyn KernelIo,
        systemd: Box<dyn SystemdIo>,
    ) -> io::Result<Self> {
        let mode = ReconcileMode::load(io, &state_dir);
        let targets = match io.read_to_string(&state_dir.join(STATE_FILE)) {
            Ok(content) => serde_json::from_str::<PersistedState>(&content)
                .map(|state| state.targets)
                .unwrap_or_default(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error),
        };
        let mut reconciler = Self {
            state_dir,
            mode,
            targets,
            previous_desired: BTreeSet::new(),
            last_ac: None,
            last_workload: WorkloadClass::Idle,
            last_mode: Mode::Auto,
            last_domain_modes: HashMap::new(),
            systemd,
        };
        reconciler.hydrate_legacy(io)?;
        reconciler.persist(io)?;
        Ok(reconciler)
    }

    pub(crate) fn mode(&self) -> ReconcileMode {
        self.mode
    }

    pub(crate) fn detect_transitions(
        &mut self,
        on_ac: Option<bool>,
        workload: WorkloadClass,
        mode: Mode,
        domain_modes: &HashMap<Domain, DomainMode>,
    ) -> Vec<Transition> {
        let mut transitions = Vec::new();
        if self.last_ac.is_some() && self.last_ac != on_ac {
            transitions.push(Transition::AcChanged {
                from: self.last_ac,
                to: on_ac,
            });
        }
        self.last_ac = on_ac;
        if self.last_workload != workload {
            transitions.push(Transition::WorkloadChanged {
                from: self.last_workload,
                to: workload,
            });
        }
        self.last_workload = workload;
        if self.last_mode != mode {
            transitions.push(Transition::ModeChanged {
                from: self.last_mode,
                to: mode,
            });
        }
        self.last_mode = mode;
        for (domain, current) in domain_modes {
            if self
                .last_domain_modes
                .get(domain)
                .is_some_and(|previous| *previous != DomainMode::Off && *current == DomainMode::Off)
            {
                transitions.push(Transition::DomainDisabled { domain: *domain });
            }
            self.last_domain_modes.insert(*domain, *current);
        }
        transitions
    }

    pub(crate) fn signal_config_reload(&self) -> Transition {
        Transition::ConfigReloaded
    }

    pub(crate) fn signal_device_removed(
        &mut self,
        domain: Domain,
        device_id: String,
    ) -> Transition {
        if let Some(state) = self.targets.get_mut(&device_id) {
            state.desired = None;
        }
        Transition::DeviceRemoved { domain, device_id }
    }

    pub(crate) fn prepare_cycle(
        &mut self,
        actions: &[Action],
        actuator: &mut Actuator,
    ) -> io::Result<Vec<String>> {
        self.previous_desired = self
            .targets
            .iter()
            .filter(|(_, state)| state.desired.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for state in self.targets.values_mut() {
            state.desired = None;
        }

        let mut desired_ids = BTreeSet::new();
        for action in actions {
            for desired in self.expand_action(action, actuator)? {
                desired_ids.insert(desired.target_id.clone());
                let baseline = self.read_target(actuator, &desired.target).ok();
                let state = self
                    .targets
                    .entry(desired.target_id.clone())
                    .or_insert_with(|| TargetState {
                        target_id: desired.target_id.clone(),
                        domain: desired.domain.clone(),
                        target: desired.target.clone(),
                        legacy_journal_key: desired.legacy_journal_key.clone(),
                        baseline: baseline.clone(),
                        desired: None,
                        last_attempted: None,
                        last_confirmed: None,
                        ownership: OwnershipState::Unowned,
                        ownership_reason: None,
                        retries: 0,
                        restore_pending: false,
                    });
                state.domain = desired.domain;
                state.target = desired.target;
                state.legacy_journal_key = desired.legacy_journal_key;
                if state.baseline.is_none() {
                    state.baseline = baseline;
                }
                if state.desired.as_ref() != Some(&desired.desired) {
                    state.retries = 0;
                }
                state.desired = Some(desired.desired);
            }
        }
        self.persist(actuator.kernel.as_ref())?;

        Ok(self
            .previous_desired
            .difference(&desired_ids)
            .cloned()
            .collect())
    }

    pub(crate) fn apply_action(
        &mut self,
        actuator: &mut Actuator,
        action: &Action,
    ) -> io::Result<ActionOutcome> {
        if actuator
            .boot_state
            .as_ref()
            .is_some_and(|boot| !boot.apply_armed)
        {
            let mut outcome = ActionOutcome::new(action);
            outcome.gates.push(GateEvaluation::allowed(
                GateStage::DomainMode,
                GateReasonCode::DomainActuate,
            ));
            outcome.gates.push(GateEvaluation::denied(
                GateStage::ApplyArmed,
                GateReasonCode::ApplyDisarmedByBootState,
                "dynamic writes are disarmed",
            ));
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::ApplyGate,
                "dynamic writes are disarmed".to_string(),
            ));
            return Ok(outcome);
        }
        let expanded = self.expand_action(action, actuator).unwrap_or_default();
        if expanded.iter().any(|desired| {
            self.targets
                .get(&desired.target_id)
                .is_none_or(|state| state.baseline.is_none())
        }) {
            let mut outcome = ActionOutcome::new(action);
            outcome.gates.push(GateEvaluation::allowed(
                GateStage::DomainMode,
                GateReasonCode::DomainActuate,
            ));
            outcome.gates.push(GateEvaluation::denied(
                GateStage::RecoveryJournal,
                GateReasonCode::JournalFailed,
                "baseline capture failed; write refused",
            ));
            for desired in expanded {
                outcome.targets.push(TargetOutcome::denied(
                    desired.target_id,
                    PipelineStage::Journal,
                    "baseline capture failed; write refused".to_string(),
                ));
            }
            return Ok(outcome);
        }

        let outcome = match action {
            Action::SystemdSetProperty {
                unit, properties, ..
            } => self.apply_systemd_action(actuator, action, unit, properties)?,
            _ if self.action_is_coalesced(action, actuator) => {
                let mut outcome = ActionOutcome::new(action);
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::DomainMode,
                    GateReasonCode::DomainActuate,
                ));
                outcome.targets.push(TargetOutcome {
                    target_id: action.stable_target_id(),
                    pipeline_stage: PipelineStage::Write,
                    support: SupportState::Supported,
                    reason: OutcomeReasonCode::RedundantValue,
                    write_attempted: false,
                    write_outcome: WriteOutcome::Redundant,
                    readback: ReadbackOutcome::NotPerformed,
                    ownership: OwnershipState::Optid,
                    pending_restore: RestoreState::Pending,
                    responsible_subsystem: ResponsibleSubsystem::Restoration,
                    detail: Some("complete desired state already confirmed".to_string()),
                });
                outcome
            }
            _ => actuator.apply(action)?,
        };
        self.record_action_outcome(action, &outcome, actuator)?;
        Ok(outcome)
    }

    pub(crate) fn reconcile(
        &mut self,
        actuator: &mut Actuator,
    ) -> io::Result<Vec<RestoreOutcome>> {
        let plans = self.plan_restores();
        let mut outcomes = Vec::with_capacity(plans.len());
        for plan in plans {
            let outcome = if self.mode == ReconcileMode::Shadow {
                RestoreOutcome {
                    target_id: plan.target_id.clone(),
                    pipeline_stage: PipelineStage::Restore,
                    reason: OutcomeReasonCode::NotEvaluated,
                    write_attempted: false,
                    write_outcome: WriteOutcome::NotEvaluated,
                    readback: ReadbackOutcome::NotPerformed,
                    ownership: OwnershipState::Optid,
                    pending_restore: RestoreState::Pending,
                    responsible_subsystem: ResponsibleSubsystem::Restoration,
                    detail: Some("shadow restore plan; no write executed".to_string()),
                }
            } else {
                actuator.execute_restore(&plan, self.systemd.as_ref())?
            };
            self.record_restore_outcome(&plan, &outcome, actuator.kernel.as_ref());
            outcomes.push(outcome);
        }
        self.persist(actuator.kernel.as_ref())?;
        Ok(outcomes)
    }

    pub(crate) fn restore_all_owned(
        &mut self,
        actuator: &mut Actuator,
    ) -> io::Result<Vec<RestoreOutcome>> {
        for state in self.targets.values_mut() {
            state.desired = None;
        }
        self.reconcile(actuator)
    }

    pub(crate) fn parity_report(&self, legacy_stale_keys: &BTreeSet<String>) -> ParityReport {
        let plans = self.plan_restores();
        let planned: BTreeSet<String> = plans
            .iter()
            .filter_map(|plan| plan.legacy_journal_key.clone())
            .collect();
        let comparable_planned: BTreeSet<String> = planned
            .iter()
            .filter(|key| legacy_restore_supported(key))
            .cloned()
            .collect();
        let comparable_legacy: BTreeSet<String> = legacy_stale_keys
            .iter()
            .filter(|key| legacy_restore_supported(key))
            .cloned()
            .collect();
        ParityReport {
            legacy: comparable_legacy.clone(),
            v1: comparable_planned.clone(),
            parity: comparable_legacy == comparable_planned,
            intentional_v1_only: plans
                .iter()
                .filter(|plan| {
                    plan.legacy_journal_key
                        .as_deref()
                        .is_none_or(|key| !legacy_restore_supported(key))
                })
                .map(|plan| plan.target_id.clone())
                .collect(),
        }
    }

    fn action_is_coalesced(&self, action: &Action, actuator: &mut Actuator) -> bool {
        let Ok(targets) = self.expand_action(action, actuator) else {
            return false;
        };
        !targets.is_empty()
            && targets.iter().all(|target| {
                self.targets.get(&target.target_id).is_some_and(|state| {
                    state.ownership == OwnershipState::Optid
                        && state.last_confirmed.as_ref() == Some(&target.desired)
                })
            })
    }

    fn record_action_outcome(
        &mut self,
        action: &Action,
        outcome: &ActionOutcome,
        actuator: &mut Actuator,
    ) -> io::Result<()> {
        let desired_targets = self.expand_action(action, actuator)?;
        let by_id: HashMap<&str, &TargetOutcome> = outcome
            .targets
            .iter()
            .map(|target| (target.target_id.as_str(), target))
            .collect();
        for desired in desired_targets {
            let Some(state) = self.targets.get_mut(&desired.target_id) else {
                continue;
            };
            let target_outcome = by_id
                .get(desired.target_id.as_str())
                .copied()
                .or_else(|| outcome.targets.first());
            let Some(target_outcome) = target_outcome else {
                continue;
            };
            if target_outcome.write_attempted {
                state.last_attempted = Some(desired.desired.clone());
            }
            match (&target_outcome.readback, target_outcome.write_attempted) {
                (ReadbackOutcome::Confirmed { .. }, true)
                    if target_outcome.ownership == OwnershipState::Optid =>
                {
                    if state.baseline.is_none() {
                        state.ownership = OwnershipState::Unowned;
                        state.ownership_reason = Some(
                            "write confirmed but baseline is missing; ownership not claimed".to_string(),
                        );
                        state.restore_pending = false;
                        continue;
                    }
                    state.last_confirmed = Some(desired.desired);
                    state.ownership = OwnershipState::Optid;
                    state.ownership_reason = None;
                    state.restore_pending = true;
                    state.retries = 0;
                }
                (ReadbackOutcome::Mismatch { expected, actual }, _) => {
                    state.ownership = OwnershipState::Relinquished;
                    state.ownership_reason = Some(format!(
                        "apply readback drift: expected {expected}, observed {actual}"
                    ));
                    state.restore_pending = false;
                }
                _ => {}
            }
        }
        self.persist(actuator.kernel.as_ref())
    }

    fn record_restore_outcome(
        &mut self,
        plan: &RestorePlan,
        outcome: &RestoreOutcome,
        io: &dyn KernelIo,
    ) {
        let Some(state) = self.targets.get_mut(&plan.target_id) else {
            return;
        };
        match &outcome.write_outcome {
            WriteOutcome::Restored => {
                state.ownership = OwnershipState::Unowned;
                state.ownership_reason = None;
                state.last_attempted = Some(plan.baseline.clone());
                state.last_confirmed = None;
                state.restore_pending = false;
                state.retries = 0;
                if let Some(key) = &plan.legacy_journal_key {
                    clear_journal_with(io, &self.state_dir, key);
                }
            }
            WriteOutcome::OwnershipRelinquished => {
                state.ownership = OwnershipState::Relinquished;
                state.ownership_reason = outcome.detail.clone();
                state.restore_pending = false;
            }
            WriteOutcome::RestorationFailed { .. } => {
                state.retries = state.retries.saturating_add(1);
                if state.retries >= MAX_RESTORE_RETRIES {
                    state.ownership = OwnershipState::Relinquished;
                    state.ownership_reason = Some(format!(
                        "restore retry limit reached ({MAX_RESTORE_RETRIES})"
                    ));
                    state.restore_pending = false;
                } else {
                    state.restore_pending = true;
                }
            }
            _ => {}
        }
    }

    fn plan_restores(&self) -> Vec<RestorePlan> {
        self.targets
            .values()
            .filter(|state| {
                state.ownership == OwnershipState::Optid
                    && state.desired.is_none()
                    && state.restore_pending
                    && state.retries < MAX_RESTORE_RETRIES
            })
            .filter_map(|state| {
                Some(RestorePlan {
                    target_id: state.target_id.clone(),
                    target: state.target.clone(),
                    baseline: state.baseline.clone()?,
                    last_confirmed: state.last_confirmed.clone()?,
                    legacy_journal_key: state.legacy_journal_key.clone(),
                })
            })
            .collect()
    }

    fn expand_action(
        &self,
        action: &Action,
        actuator: &mut Actuator,
    ) -> io::Result<Vec<DesiredTarget>> {
        let domain = action
            .domain()
            .map(|domain| domain.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let legacy = action.journal_key();
        let targets = match action {
            Action::CpuEpp { value, .. } => discover_cpu_epp_paths_with(actuator.kernel.as_ref())
                .into_iter()
                .map(|path| DesiredTarget {
                    target_id: action.stable_expanded_target_id(&path),
                    domain: domain.clone(),
                    target: TargetKind::KernelValue { path },
                    desired: StoredValue::Scalar {
                        value: value.clone(),
                    },
                    legacy_journal_key: None,
                })
                .collect(),
            Action::PlatformProfile { value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue {
                    path: PathBuf::from("/sys/firmware/acpi/platform_profile"),
                },
                desired: StoredValue::Scalar {
                    value: value.clone(),
                },
                legacy_journal_key: None,
            }],
            Action::SystemdSetProperty {
                unit, properties, ..
            } => properties
                .iter()
                .filter_map(|assignment| assignment.split_once('='))
                .map(|(property, value)| DesiredTarget {
                    target_id: format!(
                        "{}:property:{}",
                        action.stable_target_id(),
                        sanitize_identity(property)
                    ),
                    domain: domain.clone(),
                    target: TargetKind::SystemdProperty {
                        unit: unit.clone(),
                        property: property.to_string(),
                    },
                    desired: StoredValue::Systemd {
                        explicit: true,
                        value: value.to_string(),
                    },
                    legacy_journal_key: None,
                })
                .collect(),
            Action::VmSysctl { path, value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue { path: path.clone() },
                desired: StoredValue::Scalar {
                    value: value.clone(),
                },
                legacy_journal_key: legacy,
            }],
            Action::CpuDmaLatency { value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::PmqosCpu,
                desired: StoredValue::Scalar {
                    value: value
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unconstrained".to_string()),
                },
                legacy_journal_key: legacy,
            }],
            Action::DeviceResumeLatency { path, value, .. } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::PmqosDevice { path: path.clone() },
                desired: StoredValue::Scalar {
                    value: value
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "0".to_string()),
                },
                legacy_journal_key: legacy,
            }],
            Action::RuntimePm {
                device_dir,
                autosuspend_delay_ms,
                ..
            } => {
                let delay_path = device_dir.join("power/autosuspend_delay_ms");
                let has_delay = actuator.kernel.exists(&delay_path);
                vec![DesiredTarget {
                    target_id: action.stable_target_id(),
                    domain,
                    target: TargetKind::RuntimePm {
                        control_path: device_dir.join("power/control"),
                        delay_path: has_delay.then_some(delay_path),
                    },
                    desired: StoredValue::RuntimePm {
                        control: "auto".to_string(),
                        delay: has_delay.then(|| autosuspend_delay_ms.to_string()),
                    },
                    legacy_journal_key: legacy,
                }]
            }
            Action::PcieAspm {
                device_dir, enable, ..
            } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue {
                    path: device_dir.join("link/l1_aspm"),
                },
                desired: StoredValue::Scalar {
                    value: if *enable { "1" } else { "0" }.to_string(),
                },
                legacy_journal_key: legacy,
            }],
            Action::SataAlpm {
                host_dir, policy, ..
            } => vec![DesiredTarget {
                target_id: action.stable_target_id(),
                domain,
                target: TargetKind::KernelValue {
                    path: host_dir.join("link_power_management_policy"),
                },
                desired: StoredValue::Scalar {
                    value: policy.clone(),
                },
                legacy_journal_key: legacy,
            }],
            Action::Backlight {
                device_dir,
                target_pct,
                ..
            } => {
                let max_path = device_dir.join("max_brightness");
                let max = actuator
                    .kernel
                    .read_to_string(&max_path)?
                    .trim()
                    .parse::<u64>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let value = display::compute_target_brightness(max, *target_pct).to_string();
                vec![DesiredTarget {
                    target_id: action.stable_target_id(),
                    domain,
                    target: TargetKind::KernelValue {
                        path: device_dir.join("brightness"),
                    },
                    desired: StoredValue::Scalar { value },
                    legacy_journal_key: legacy,
                }]
            }
        };
        Ok(targets)
    }

    fn apply_systemd_action(
        &mut self,
        _actuator: &mut Actuator,
        action: &Action,
        unit: &str,
        properties: &[String],
    ) -> io::Result<ActionOutcome> {
        let mut outcome = ActionOutcome::new(action);
        outcome.gates.push(GateEvaluation::allowed(
            GateStage::DomainMode,
            GateReasonCode::DomainActuate,
        ));
        outcome.gates.push(GateEvaluation::allowed(
            GateStage::ApplyArmed,
            GateReasonCode::ApplyArmed,
        ));
        outcome.gates.push(GateEvaluation::not_applicable(
            GateStage::Contract,
            GateReasonCode::ContractNotApplicable,
        ));
        outcome.gates.push(GateEvaluation::not_applicable(
            GateStage::CapabilityValidation,
            GateReasonCode::CapabilityAllowed,
        ));
        for assignment in properties {
            let Some((property, value)) = assignment.split_once('=') else {
                let error = io::Error::new(io::ErrorKind::InvalidInput, "invalid systemd property");
                outcome.targets.push(TargetOutcome::failed(
                    format!("{}:property:invalid", action.stable_target_id()),
                    &error,
                ));
                continue;
            };
            let target_id = format!(
                "{}:property:{}",
                action.stable_target_id(),
                sanitize_identity(property)
            );
            let current = self.systemd.read_property(unit, property)?;
            if current.explicit && current.value == value {
                outcome.targets.push(TargetOutcome {
                    target_id: target_id.clone(),
                    pipeline_stage: PipelineStage::Write,
                    support: SupportState::Supported,
                    reason: OutcomeReasonCode::RedundantValue,
                    write_attempted: false,
                    write_outcome: WriteOutcome::Redundant,
                    readback: ReadbackOutcome::Confirmed {
                        value: current.value,
                    },
                    ownership: self
                        .targets
                        .get(&target_id)
                        .map(|state| state.ownership.clone())
                        .unwrap_or(OwnershipState::Unowned),
                    pending_restore: RestoreState::Pending,
                    responsible_subsystem: ResponsibleSubsystem::Systemd,
                    detail: None,
                });
                continue;
            }
            match self.systemd.set_property(unit, property, Some(value)) {
                Ok(()) => {
                    let readback = self.systemd.read_property(unit, property)?;
                    let confirmed = readback.explicit && readback.value == value;
                    outcome.targets.push(TargetOutcome {
                        target_id,
                        pipeline_stage: PipelineStage::Readback,
                        support: SupportState::Supported,
                        reason: if confirmed {
                            OutcomeReasonCode::ReadbackConfirmed
                        } else {
                            OutcomeReasonCode::ReadbackMismatch
                        },
                        write_attempted: true,
                        write_outcome: WriteOutcome::Applied,
                        readback: if confirmed {
                            ReadbackOutcome::Confirmed {
                                value: readback.value,
                            }
                        } else {
                            ReadbackOutcome::Mismatch {
                                expected: value.to_string(),
                                actual: readback.value,
                            }
                        },
                        ownership: if confirmed {
                            OwnershipState::Optid
                        } else {
                            OwnershipState::Drifted
                        },
                        pending_restore: RestoreState::Pending,
                        responsible_subsystem: ResponsibleSubsystem::Systemd,
                        detail: None,
                    });
                }
                Err(error) => outcome
                    .targets
                    .push(TargetOutcome::failed(target_id, &error)),
            }
        }
        Ok(outcome)
    }

    fn read_target(&self, actuator: &mut Actuator, target: &TargetKind) -> io::Result<StoredValue> {
        match target {
            TargetKind::KernelValue { path } => Ok(StoredValue::Scalar {
                value: actuator.kernel.read_to_string(path)?.trim().to_string(),
            }),
            TargetKind::PmqosCpu => Ok(StoredValue::Scalar {
                value: actuator.pmqos_sink.read_cpu_latency()?.trim().to_string(),
            }),
            TargetKind::PmqosDevice { path } => Ok(StoredValue::Scalar {
                value: actuator
                    .pmqos_sink
                    .read_device_latency(path)?
                    .trim()
                    .to_string(),
            }),
            TargetKind::RuntimePm {
                control_path,
                delay_path,
            } => Ok(StoredValue::RuntimePm {
                control: actuator
                    .kernel
                    .read_to_string(control_path)?
                    .trim()
                    .to_string(),
                delay: delay_path
                    .as_ref()
                    .map(|path| {
                        actuator
                            .kernel
                            .read_to_string(path)
                            .map(|value| value.trim().to_string())
                    })
                    .transpose()?,
            }),
            TargetKind::SystemdProperty { unit, property } => {
                let state = self.systemd.read_property(unit, property)?;
                Ok(StoredValue::Systemd {
                    explicit: state.explicit,
                    value: state.value,
                })
            }
        }
    }

    fn persist(&self, io: &dyn KernelIo) -> io::Result<()> {
        let content = serde_json::to_string_pretty(&PersistedState {
            schema_version: 1,
            targets: self.targets.clone(),
        })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        atomic_write_state_file_with(io, &self.state_dir.join(STATE_FILE), &content)
    }

    fn hydrate_legacy(&mut self, io: &dyn KernelIo) -> io::Result<()> {
        let entries = match io.read_dir(&self.state_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for original_path in entries {
            let Some(name) = original_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(key) = name.strip_prefix("original_") else {
                continue;
            };
            if self
                .targets
                .values()
                .any(|state| state.legacy_journal_key.as_deref() == Some(key))
            {
                continue;
            }
            let Some((target_id, domain, target, baseline)) =
                parse_legacy_original(io, key, &original_path)?
            else {
                continue;
            };
            let applied_path = self.state_dir.join(format!("applied_{key}"));
            let last_confirmed = io
                .read_to_string(&applied_path)
                .ok()
                .and_then(|content| parse_legacy_applied(key, &content));
            let current = read_target_with(io, &target).ok();
            let ownership = if last_confirmed.is_some() && current == last_confirmed {
                OwnershipState::Optid
            } else {
                OwnershipState::Unowned
            };
            self.targets.insert(
                target_id.clone(),
                TargetState {
                    target_id,
                    domain,
                    target,
                    legacy_journal_key: Some(key.to_string()),
                    baseline: Some(baseline),
                    desired: None,
                    last_attempted: last_confirmed.clone(),
                    last_confirmed,
                    ownership: ownership.clone(),
                    ownership_reason: None,
                    retries: 0,
                    restore_pending: ownership == OwnershipState::Optid,
                },
            );
        }
        Ok(())
    }
}

impl Actuator {
    pub(crate) fn execute_restore(
        &mut self,
        plan: &RestorePlan,
        systemd: &dyn SystemdIo,
    ) -> io::Result<RestoreOutcome> {
        let current = read_target_for_restore(self, systemd, &plan.target);
        let current = match current {
            Ok(current) => current,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(relinquished_outcome(
                    &plan.target_id,
                    "target disappeared before restoration",
                ));
            }
            Err(error) => return Ok(failed_restore(&plan.target_id, false, &error)),
        };
        if current != plan.last_confirmed {
            return Ok(RestoreOutcome {
                target_id: plan.target_id.clone(),
                pipeline_stage: PipelineStage::Restore,
                reason: OutcomeReasonCode::OwnershipRelinquished,
                write_attempted: false,
                write_outcome: WriteOutcome::OwnershipRelinquished,
                readback: ReadbackOutcome::Mismatch {
                    expected: plan.last_confirmed.public_value(),
                    actual: current.public_value(),
                },
                ownership: OwnershipState::Relinquished,
                pending_restore: RestoreState::NotApplicable,
                responsible_subsystem: ResponsibleSubsystem::Restoration,
                detail: Some("external drift detected; restore refused".to_string()),
            });
        }

        if let Err(error) = write_target_for_restore(self, systemd, &plan.target, &plan.baseline) {
            return Ok(failed_restore(&plan.target_id, true, &error));
        }
        let readback = match read_target_for_restore(self, systemd, &plan.target) {
            Ok(readback) => readback,
            Err(error) => return Ok(failed_restore(&plan.target_id, true, &error)),
        };
        if readback != plan.baseline {
            return Ok(RestoreOutcome {
                target_id: plan.target_id.clone(),
                pipeline_stage: PipelineStage::Readback,
                reason: OutcomeReasonCode::RestoreFailed,
                write_attempted: true,
                write_outcome: WriteOutcome::RestorationFailed {
                    error_kind: ErrorKindCode::Other,
                },
                readback: ReadbackOutcome::Mismatch {
                    expected: plan.baseline.public_value(),
                    actual: readback.public_value(),
                },
                ownership: OwnershipState::Optid,
                pending_restore: RestoreState::Pending,
                responsible_subsystem: ResponsibleSubsystem::Restoration,
                detail: Some("restore readback mismatch".to_string()),
            });
        }
        Ok(RestoreOutcome {
            target_id: plan.target_id.clone(),
            pipeline_stage: PipelineStage::Restore,
            reason: OutcomeReasonCode::RestoreApplied,
            write_attempted: true,
            write_outcome: WriteOutcome::Restored,
            readback: ReadbackOutcome::Confirmed {
                value: readback.public_value(),
            },
            ownership: OwnershipState::Unowned,
            pending_restore: RestoreState::Restored,
            responsible_subsystem: ResponsibleSubsystem::Restoration,
            detail: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParityReport {
    pub(crate) legacy: BTreeSet<String>,
    pub(crate) v1: BTreeSet<String>,
    pub(crate) parity: bool,
    pub(crate) intentional_v1_only: BTreeSet<String>,
}

fn read_target_for_restore(
    actuator: &mut Actuator,
    systemd: &dyn SystemdIo,
    target: &TargetKind,
) -> io::Result<StoredValue> {
    match target {
        TargetKind::KernelValue { path } => Ok(StoredValue::Scalar {
            value: actuator.kernel.read_to_string(path)?.trim().to_string(),
        }),
        TargetKind::PmqosCpu => Ok(StoredValue::Scalar {
            value: actuator.pmqos_sink.read_cpu_latency()?.trim().to_string(),
        }),
        TargetKind::PmqosDevice { path } => Ok(StoredValue::Scalar {
            value: actuator
                .pmqos_sink
                .read_device_latency(path)?
                .trim()
                .to_string(),
        }),
        TargetKind::RuntimePm {
            control_path,
            delay_path,
        } => Ok(StoredValue::RuntimePm {
            control: actuator
                .kernel
                .read_to_string(control_path)?
                .trim()
                .to_string(),
            delay: delay_path
                .as_ref()
                .map(|path| {
                    actuator
                        .kernel
                        .read_to_string(path)
                        .map(|value| value.trim().to_string())
                })
                .transpose()?,
        }),
        TargetKind::SystemdProperty { unit, property } => {
            let state = systemd.read_property(unit, property)?;
            Ok(StoredValue::Systemd {
                explicit: state.explicit,
                value: state.value,
            })
        }
    }
}

fn write_target_for_restore(
    actuator: &mut Actuator,
    systemd: &dyn SystemdIo,
    target: &TargetKind,
    value: &StoredValue,
) -> io::Result<()> {
    match (target, value) {
        (TargetKind::KernelValue { path }, StoredValue::Scalar { value }) => {
            actuator.kernel.write(path, value)
        }
        (TargetKind::PmqosCpu, StoredValue::Scalar { value }) => {
            let parsed = if value == "unconstrained" {
                None
            } else {
                Some(
                    value
                        .parse::<i32>()
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
                )
            };
            actuator.pmqos_sink.write_cpu_latency(parsed)
        }
        (TargetKind::PmqosDevice { path }, StoredValue::Scalar { value }) => {
            actuator.pmqos_sink.write_device_latency(path, value)
        }
        (
            TargetKind::RuntimePm {
                control_path,
                delay_path,
            },
            StoredValue::RuntimePm { control, delay },
        ) => {
            actuator.kernel.write(control_path, control)?;
            if let (Some(path), Some(delay)) = (delay_path, delay) {
                actuator.kernel.write(path, delay)?;
            }
            Ok(())
        }
        (
            TargetKind::SystemdProperty { unit, property },
            StoredValue::Systemd { explicit, value },
        ) => systemd.set_property(unit, property, explicit.then_some(value.as_str())),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "target/value kind mismatch",
        )),
    }
}

fn read_target_with(io: &dyn KernelIo, target: &TargetKind) -> io::Result<StoredValue> {
    match target {
        TargetKind::KernelValue { path } => Ok(StoredValue::Scalar {
            value: io.read_to_string(path)?.trim().to_string(),
        }),
        TargetKind::RuntimePm {
            control_path,
            delay_path,
        } => Ok(StoredValue::RuntimePm {
            control: io.read_to_string(control_path)?.trim().to_string(),
            delay: delay_path
                .as_ref()
                .map(|path| io.read_to_string(path).map(|value| value.trim().to_string()))
                .transpose()?,
        }),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "legacy hydration does not support this target kind",
        )),
    }
}

fn parse_legacy_original(
    io: &dyn KernelIo,
    key: &str,
    path: &Path,
) -> io::Result<Option<(String, String, TargetKind, StoredValue)>> {
    let content = io.read_to_string(path)?;
    let mut lines = content.lines();
    let parsed = if let Some(hash) = key.strip_prefix("rpm_") {
        let Some(device_dir) = lines.next() else { return Ok(None) };
        let Some(control) = lines.next() else { return Ok(None) };
        let delay = lines
            .next()
            .filter(|value| *value != "n/a")
            .map(str::to_string);
        let device_dir = PathBuf::from(device_dir);
        (
            format!("runtime-pm:{hash}"),
            "runtime_pm".to_string(),
            TargetKind::RuntimePm {
                control_path: device_dir.join("power/control"),
                delay_path: delay
                    .as_ref()
                    .map(|_| device_dir.join("power/autosuspend_delay_ms")),
            },
            StoredValue::RuntimePm {
                control: control.trim().to_string(),
                delay,
            },
        )
    } else if let Some(hash) = key.strip_prefix("dev_") {
        let Some(attr) = lines.next() else { return Ok(None) };
        let Some(value) = lines.next() else { return Ok(None) };
        (
            format!("device-resume:{hash}"),
            "device_resume_latency".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(attr),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(hash) = key.strip_prefix("aspm_") {
        let Some(base) = lines.next() else { return Ok(None) };
        let Some(value) = lines.next() else { return Ok(None) };
        (
            format!("pcie-aspm:{hash}"),
            "pci_aspm".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(base).join("link/l1_aspm"),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(hash) = key.strip_prefix("alpm_") {
        let Some(base) = lines.next() else { return Ok(None) };
        let Some(value) = lines.next() else { return Ok(None) };
        (
            format!("sata-alpm:{hash}"),
            "sata_alpm".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(base).join("link_power_management_policy"),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(hash) = key.strip_prefix("bl_") {
        let Some(base) = lines.next() else { return Ok(None) };
        let Some(value) = lines.next() else { return Ok(None) };
        (
            format!("backlight:{hash}"),
            "backlight".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(base).join("brightness"),
            },
            StoredValue::Scalar {
                value: value.trim().to_string(),
            },
        )
    } else if let Some(name) = key.strip_prefix("vm_") {
        (
            format!("vm-sysctl:{}", sanitize_identity(name)),
            "vm_sysctl".to_string(),
            TargetKind::KernelValue {
                path: PathBuf::from(format!("/proc/sys/vm/{name}")),
            },
            StoredValue::Scalar {
                value: content.trim().to_string(),
            },
        )
    } else {
        return Ok(None);
    };
    Ok(Some(parsed))
}

fn parse_legacy_applied(key: &str, content: &str) -> Option<StoredValue> {
    let value = content.split_once('\n')?.1;
    if key.starts_with("rpm_") {
        let mut lines = value.lines();
        Some(StoredValue::RuntimePm {
            control: lines.next()?.trim().to_string(),
            delay: lines.next().map(|value| value.trim().to_string()),
        })
    } else {
        Some(StoredValue::Scalar {
            value: value.trim().to_string(),
        })
    }
}

fn failed_restore(target_id: &str, attempted: bool, error: &io::Error) -> RestoreOutcome {
    RestoreOutcome {
        target_id: target_id.to_string(),
        pipeline_stage: PipelineStage::Restore,
        reason: OutcomeReasonCode::RestoreFailed,
        write_attempted: attempted,
        write_outcome: WriteOutcome::RestorationFailed {
            error_kind: ErrorKindCode::from_io(error),
        },
        readback: ReadbackOutcome::NotPerformed,
        ownership: OwnershipState::Optid,
        pending_restore: RestoreState::Pending,
        responsible_subsystem: ResponsibleSubsystem::Restoration,
        detail: Some(format!("restore failed: {:?}", error.kind())),
    }
}

fn relinquished_outcome(target_id: &str, detail: &str) -> RestoreOutcome {
    RestoreOutcome {
        target_id: target_id.to_string(),
        pipeline_stage: PipelineStage::Restore,
        reason: OutcomeReasonCode::OwnershipRelinquished,
        write_attempted: false,
        write_outcome: WriteOutcome::OwnershipRelinquished,
        readback: ReadbackOutcome::Unavailable,
        ownership: OwnershipState::Relinquished,
        pending_restore: RestoreState::NotApplicable,
        responsible_subsystem: ResponsibleSubsystem::Restoration,
        detail: Some(detail.to_string()),
    }
}

fn legacy_restore_supported(key: &str) -> bool {
    ["rpm_", "dev_", "aspm_", "alpm_", "bl_"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

fn sanitize_identity(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
