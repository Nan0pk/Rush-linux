//! Production F3 control-cycle envelope.
//!
//! The daemon creates exactly one [`ControlCycleEnvelope`] per control-loop
//! iteration and uses that same value for `status.json`, `control-cycles.jsonl`,
//! D-Bus JSON status, and correlation-aware text status. The schema is public
//! diagnostic data: it uses stable logical identities and deliberately excludes
//! raw kernel paths, source locations, command lines, environment variables,
//! credentials, usernames, and home directories.
//!
//! Compatibility policy:
//! - unknown object fields are ignored by serde readers;
//! - optional fields default to `None` or an empty collection;
//! - enum values are closed and an unknown future value fails clearly;
//! - `schema_version` changes only when field meaning or enum semantics break.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::decision::Decision;
use crate::load_state::BootState;
use crate::policy::{Domain, DomainMode};
use crate::sensors::{ObservationFailureKind, Pressure, Snapshot};

pub(crate) const ENVELOPE_SCHEMA_VERSION: u32 = 2;
pub(crate) type CorrelationId = String;

#[derive(Debug, Clone)]
pub(crate) struct CycleIdGenerator {
    boot_scope: u64,
    sequence: u64,
}

impl CycleIdGenerator {
    pub(crate) fn new(boot_scope: u64) -> Self {
        Self {
            boot_scope,
            sequence: 0,
        }
    }

    pub(crate) fn next(&mut self) -> CorrelationId {
        let id = format!("optid-{:016x}-{:016x}", self.boot_scope, self.sequence);
        self.sequence = self.sequence.wrapping_add(1);
        id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PipelineStage {
    Observation,
    Decision,
    DomainGate,
    ApplyGate,
    ContractGate,
    AllowlistGate,
    CapabilityGate,
    Journal,
    Write,
    Readback,
    Restore,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponsibleSubsystem {
    Sensors,
    Policy,
    Daemon,
    Actuator,
    KernelIo,
    Systemd,
    Restoration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupportState {
    Supported,
    Unsupported,
    NotApplicable,
    NotEvaluated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateStage {
    DomainMode,
    ApplyArmed,
    Contract,
    HardwareAllowlist,
    CapabilityValidation,
    RecoveryJournal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateDisposition {
    Allowed,
    Denied,
    NotApplicable,
    NotEvaluated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateReasonCode {
    DomainActuate,
    DomainOff,
    DomainObserve,
    ApplyArmed,
    ApplyNotRequested,
    ApplyDisarmedByBootState,
    ContractAllowed,
    ContractNotApplicable,
    ContractMissing,
    ContractFloorInvalid,
    ContractDenied,
    AllowlistDisabled,
    AllowlistAllowed,
    AllowlistDenied,
    HwidUnresolved,
    CapabilityAllowed,
    CapabilityDenied,
    JournalSucceeded,
    JournalFailed,
    NotReached,
}

pub(crate) fn sanitize_public_detail(detail: impl Into<String>) -> String {
    let detail = detail.into();
    const PROHIBITED: [&str; 8] = [
        "/sys/", "/proc/", "/dev/", "/home/", "/root/", ".rs:", "RUST_", "TOKEN=",
    ];
    if PROHIBITED.iter().any(|needle| detail.contains(needle)) {
        "diagnostic detail redacted; consult local daemon logs".to_string()
    } else {
        detail
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GateEvaluation {
    pub(crate) stage: GateStage,
    pub(crate) disposition: GateDisposition,
    pub(crate) reason: GateReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

impl GateEvaluation {
    pub(crate) fn allowed(stage: GateStage, reason: GateReasonCode) -> Self {
        Self {
            stage,
            disposition: GateDisposition::Allowed,
            reason,
            detail: None,
        }
    }

    pub(crate) fn denied(
        stage: GateStage,
        reason: GateReasonCode,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            disposition: GateDisposition::Denied,
            reason,
            detail: Some(sanitize_public_detail(detail)),
        }
    }

    pub(crate) fn not_applicable(stage: GateStage, reason: GateReasonCode) -> Self {
        Self {
            stage,
            disposition: GateDisposition::NotApplicable,
            reason,
            detail: None,
        }
    }

    pub(crate) fn not_evaluated(stage: GateStage) -> Self {
        Self {
            stage,
            disposition: GateDisposition::NotEvaluated,
            reason: GateReasonCode::NotReached,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeReasonCode {
    NotEvaluated,
    UnsupportedTarget,
    MissingTarget,
    NotApplicable,
    GateDenied,
    NetworkCarrierUp,
    RedundantValue,
    WriteApplied,
    WriteFailed,
    ReadbackConfirmed,
    ReadbackUnavailable,
    ReadbackMismatch,
    RestoreApplied,
    RestoreFailed,
    OwnershipRelinquished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKindCode {
    NotFound,
    PermissionDenied,
    InvalidData,
    Interrupted,
    Other,
}

impl ErrorKindCode {
    pub(crate) fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => Self::InvalidData,
            std::io::ErrorKind::Interrupted => Self::Interrupted,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WriteOutcome {
    NotEvaluated,
    Unsupported,
    NotApplicable,
    Denied,
    Skipped,
    Redundant,
    Applied,
    Failed { error_kind: ErrorKindCode },
    Restored,
    RestorationFailed { error_kind: ErrorKindCode },
    OwnershipRelinquished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadbackOutcome {
    NotPerformed,
    Unavailable,
    Confirmed { value: String },
    Mismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OwnershipState {
    Unowned,
    Optid,
    Drifted,
    Relinquished,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestoreState {
    NotApplicable,
    NotEvaluated,
    Pending,
    Restored,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesiredAction {
    pub(crate) operation: String,
    pub(crate) value: String,
    pub(crate) target_id: String,
}

impl DesiredAction {
    pub(crate) fn from_action(action: &Action) -> Self {
        Self {
            operation: action.desired_operation().to_string(),
            value: action.desired_value(),
            target_id: action.stable_target_id(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetOutcome {
    pub(crate) target_id: String,
    pub(crate) pipeline_stage: PipelineStage,
    pub(crate) support: SupportState,
    pub(crate) reason: OutcomeReasonCode,
    pub(crate) write_attempted: bool,
    pub(crate) write_outcome: WriteOutcome,
    pub(crate) readback: ReadbackOutcome,
    pub(crate) ownership: OwnershipState,
    pub(crate) pending_restore: RestoreState,
    pub(crate) responsible_subsystem: ResponsibleSubsystem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

impl TargetOutcome {
    pub(crate) fn denied(target_id: String, stage: PipelineStage, detail: String) -> Self {
        Self {
            target_id,
            pipeline_stage: stage,
            support: SupportState::Unknown,
            reason: OutcomeReasonCode::GateDenied,
            write_attempted: false,
            write_outcome: WriteOutcome::Denied,
            readback: ReadbackOutcome::NotPerformed,
            ownership: OwnershipState::Unowned,
            pending_restore: RestoreState::NotApplicable,
            responsible_subsystem: ResponsibleSubsystem::Actuator,
            detail: Some(detail),
        }
    }

    pub(crate) fn unsupported(target_id: String, reason: OutcomeReasonCode) -> Self {
        Self {
            target_id,
            pipeline_stage: PipelineStage::CapabilityGate,
            support: SupportState::Unsupported,
            reason,
            write_attempted: false,
            write_outcome: WriteOutcome::Unsupported,
            readback: ReadbackOutcome::NotPerformed,
            ownership: OwnershipState::Unowned,
            pending_restore: RestoreState::NotApplicable,
            responsible_subsystem: ResponsibleSubsystem::KernelIo,
            detail: None,
        }
    }

    pub(crate) fn applied(target_id: String, desired: &str, readback: ReadbackOutcome) -> Self {
        let (reason, ownership) = match &readback {
            ReadbackOutcome::Confirmed { .. } => {
                (OutcomeReasonCode::ReadbackConfirmed, OwnershipState::Optid)
            }
            ReadbackOutcome::Mismatch { .. } => {
                (OutcomeReasonCode::ReadbackMismatch, OwnershipState::Drifted)
            }
            ReadbackOutcome::Unavailable | ReadbackOutcome::NotPerformed => (
                OutcomeReasonCode::ReadbackUnavailable,
                OwnershipState::Optid,
            ),
        };
        let _ = desired;
        Self {
            target_id,
            pipeline_stage: PipelineStage::Readback,
            support: SupportState::Supported,
            reason,
            write_attempted: true,
            write_outcome: WriteOutcome::Applied,
            readback,
            ownership,
            pending_restore: RestoreState::Pending,
            responsible_subsystem: ResponsibleSubsystem::KernelIo,
            detail: None,
        }
    }

    pub(crate) fn failed(target_id: String, error: &std::io::Error) -> Self {
        Self {
            target_id,
            pipeline_stage: PipelineStage::Write,
            support: SupportState::Supported,
            reason: OutcomeReasonCode::WriteFailed,
            write_attempted: true,
            write_outcome: WriteOutcome::Failed {
                error_kind: ErrorKindCode::from_io(error),
            },
            readback: ReadbackOutcome::NotPerformed,
            ownership: OwnershipState::Unknown,
            pending_restore: RestoreState::NotEvaluated,
            responsible_subsystem: ResponsibleSubsystem::KernelIo,
            detail: Some(format!("kernel I/O failed: {:?}", error.kind())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActionOutcome {
    pub(crate) domain: String,
    pub(crate) desired: DesiredAction,
    pub(crate) gates: Vec<GateEvaluation>,
    pub(crate) targets: Vec<TargetOutcome>,
}

impl ActionOutcome {
    pub(crate) fn new(action: &Action) -> Self {
        Self {
            domain: action
                .domain()
                .map(|domain| domain.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            desired: DesiredAction::from_action(action),
            gates: Vec::new(),
            targets: Vec::new(),
        }
    }

    pub(crate) fn suppressed(action: &Action, mode: DomainMode, apply_requested: bool) -> Self {
        let mut outcome = Self::new(action);
        outcome.gates.push(match mode {
            DomainMode::Actuate => {
                GateEvaluation::allowed(GateStage::DomainMode, GateReasonCode::DomainActuate)
            }
            DomainMode::Observe => GateEvaluation::denied(
                GateStage::DomainMode,
                GateReasonCode::DomainObserve,
                "domain is in observe mode",
            ),
            DomainMode::Off => GateEvaluation::denied(
                GateStage::DomainMode,
                GateReasonCode::DomainOff,
                "domain is off",
            ),
        });
        if mode == DomainMode::Actuate && !apply_requested {
            outcome.gates.push(GateEvaluation::denied(
                GateStage::ApplyArmed,
                GateReasonCode::ApplyNotRequested,
                "daemon was started without --apply",
            ));
        } else {
            outcome
                .gates
                .push(GateEvaluation::not_evaluated(GateStage::ApplyArmed));
        }
        outcome.targets.push(TargetOutcome::denied(
            action.stable_target_id(),
            if mode == DomainMode::Actuate {
                PipelineStage::ApplyGate
            } else {
                PipelineStage::DomainGate
            },
            if mode == DomainMode::Observe {
                "observe-mode would-be action".to_string()
            } else if mode == DomainMode::Off {
                "off-mode action suppressed".to_string()
            } else {
                "apply not requested".to_string()
            },
        ));
        outcome
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RestoreOutcome {
    pub(crate) target_id: String,
    pub(crate) pipeline_stage: PipelineStage,
    pub(crate) reason: OutcomeReasonCode,
    pub(crate) write_attempted: bool,
    pub(crate) write_outcome: WriteOutcome,
    pub(crate) readback: ReadbackOutcome,
    pub(crate) ownership: OwnershipState,
    pub(crate) pending_restore: RestoreState,
    pub(crate) responsible_subsystem: ResponsibleSubsystem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservationReasonCode {
    Observed,
    Unavailable,
    NotFound,
    PermissionDenied,
    InvalidData,
    Malformed,
    ReadFailed,
}

impl ObservationReasonCode {
    fn from_failure(failure: ObservationFailureKind) -> Self {
        match failure {
            ObservationFailureKind::NotFound => Self::NotFound,
            ObservationFailureKind::PermissionDenied => Self::PermissionDenied,
            ObservationFailureKind::InvalidData => Self::InvalidData,
            ObservationFailureKind::Malformed => Self::Malformed,
            ObservationFailureKind::Other => Self::ReadFailed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ObservationValue {
    pub(crate) component_id: String,
    pub(crate) pipeline_stage: PipelineStage,
    pub(crate) support: SupportState,
    pub(crate) reason: ObservationReasonCode,
    pub(crate) value: serde_json::Value,
    pub(crate) source: String,
    pub(crate) provenance: String,
    pub(crate) responsible_subsystem: ResponsibleSubsystem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ObservationEnvelope {
    pub(crate) values: Vec<ObservationValue>,
}

impl ObservationEnvelope {
    pub(crate) fn from_snapshot(snapshot: &Snapshot) -> Self {
        fn pressure(value: Option<Pressure>) -> serde_json::Value {
            value
                .map(|pressure| {
                    serde_json::json!({
                        "avg10": pressure.avg10,
                        "avg60": pressure.avg60,
                        "avg300": pressure.avg300,
                        "total": pressure.total,
                    })
                })
                .unwrap_or(serde_json::Value::Null)
        }

        fn observation(
            snapshot: &Snapshot,
            component_id: &str,
            value: serde_json::Value,
            source: &str,
            provenance: &str,
        ) -> ObservationValue {
            let reason = snapshot
                .observation_failures
                .get(component_id)
                .copied()
                .map(ObservationReasonCode::from_failure)
                .unwrap_or_else(|| {
                    if value.is_null() {
                        ObservationReasonCode::Unavailable
                    } else {
                        ObservationReasonCode::Observed
                    }
                });
            let support = if reason == ObservationReasonCode::Observed {
                SupportState::Supported
            } else {
                SupportState::Unknown
            };
            ObservationValue {
                component_id: component_id.to_string(),
                pipeline_stage: PipelineStage::Observation,
                support,
                reason,
                value,
                source: source.to_string(),
                provenance: provenance.to_string(),
                responsible_subsystem: ResponsibleSubsystem::Sensors,
            }
        }

        let values = vec![
            observation(
                snapshot,
                "power-source",
                serde_json::json!(snapshot.on_ac),
                "power_supply",
                "kernel_interface",
            ),
            observation(
                snapshot,
                "battery-charge",
                serde_json::json!(snapshot.battery_pct),
                "power_supply",
                "kernel_interface",
            ),
            observation(
                snapshot,
                "thermal-budget",
                serde_json::json!(snapshot.thermal_c()),
                "thermal",
                "resolved_sensor_budget",
            ),
            observation(
                snapshot,
                "load-average",
                serde_json::json!(snapshot.loadavg_1),
                "procfs",
                "kernel_interface",
            ),
            observation(
                snapshot,
                "cpu-pressure",
                pressure(snapshot.cpu_pressure),
                "psi",
                "procfs",
            ),
            observation(
                snapshot,
                "memory-pressure",
                pressure(snapshot.memory_pressure),
                "psi",
                "procfs",
            ),
            observation(
                snapshot,
                "io-pressure",
                pressure(snapshot.io_pressure),
                "psi",
                "procfs",
            ),
            observation(
                snapshot,
                "zram-swap",
                serde_json::json!(snapshot.zram_swap_active),
                "procfs",
                "kernel_interface",
            ),
            observation(
                snapshot,
                "virtual-machine",
                serde_json::json!(snapshot.is_vm_guest),
                "dmi",
                "platform_identity",
            ),
        ];
        Self { values }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContractIdentity {
    pub(crate) workload_class: String,
    pub(crate) cpu_wakeup_latency_us: Option<i64>,
    pub(crate) device_resume_latency_us: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecisionEnvelope {
    pub(crate) selected_mode: String,
    pub(crate) workload_class: String,
    pub(crate) workload_reason: String,
    pub(crate) reasons: Vec<String>,
    pub(crate) contract: ContractIdentity,
}

impl DecisionEnvelope {
    pub(crate) fn from_decision(decision: &Decision) -> Self {
        Self {
            selected_mode: decision.mode.to_string(),
            workload_class: decision.workload_class.to_string(),
            workload_reason: sanitize_public_detail(decision.workload_reason.clone()),
            reasons: decision
                .reasons
                .iter()
                .cloned()
                .map(sanitize_public_detail)
                .collect(),
            contract: ContractIdentity {
                workload_class: decision.workload_class.to_string(),
                cpu_wakeup_latency_us: decision.cpu_wakeup_latency,
                device_resume_latency_us: decision.device_resume_latency,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DomainCycleRecord {
    pub(crate) domain: String,
    pub(crate) pipeline_stage: PipelineStage,
    pub(crate) selected_mode: DomainMode,
    pub(crate) support: SupportState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) component_id: Option<String>,
    #[serde(default)]
    pub(crate) action_outcomes: Vec<ActionOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BootEnvelope {
    pub(crate) policy_load_state: String,
    pub(crate) allowlist_load_state: String,
    pub(crate) allowlist_gate_enabled: bool,
    pub(crate) apply_armed: bool,
    pub(crate) baseline_armed: bool,
}

impl BootEnvelope {
    pub(crate) fn from_boot_state(boot: &BootState) -> Self {
        Self {
            policy_load_state: boot.policy_load_state.to_string(),
            allowlist_load_state: boot.allowlist_load_state.to_string(),
            allowlist_gate_enabled: boot.allowlist_gate_enabled,
            apply_armed: boot.apply_armed,
            baseline_armed: boot.baseline_armed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ControlCycleEnvelope {
    pub(crate) schema_version: u32,
    pub(crate) correlation_id: CorrelationId,
    pub(crate) cycle_timestamp: u64,
    pub(crate) pipeline_stage: PipelineStage,
    pub(crate) boot: BootEnvelope,
    pub(crate) observation: ObservationEnvelope,
    pub(crate) decision: DecisionEnvelope,
    pub(crate) domains: Vec<DomainCycleRecord>,
    #[serde(default)]
    pub(crate) restore_outcomes: Vec<RestoreOutcome>,
    pub(crate) recovery_state: String,
}

impl ControlCycleEnvelope {
    pub(crate) fn build(
        correlation_id: CorrelationId,
        snapshot: &Snapshot,
        decision: &Decision,
        boot: &BootState,
        action_outcomes: Vec<ActionOutcome>,
        restore_outcomes: Vec<RestoreOutcome>,
    ) -> Self {
        let mut by_domain: BTreeMap<&'static str, Vec<ActionOutcome>> = BTreeMap::new();
        for outcome in action_outcomes {
            let key = Domain::all()
                .iter()
                .find(|domain| domain.as_str() == outcome.domain)
                .map(Domain::as_str)
                .unwrap_or("unknown");
            by_domain.entry(key).or_default().push(outcome);
        }
        let domains = Domain::all()
            .iter()
            .map(|domain| {
                let outcomes = by_domain.remove(domain.as_str()).unwrap_or_default();
                let support = if outcomes.is_empty() {
                    SupportState::NotEvaluated
                } else if outcomes
                    .iter()
                    .flat_map(|outcome| &outcome.targets)
                    .any(|target| target.support == SupportState::Supported)
                {
                    SupportState::Supported
                } else if outcomes
                    .iter()
                    .flat_map(|outcome| &outcome.targets)
                    .any(|target| target.support == SupportState::Unsupported)
                {
                    SupportState::Unsupported
                } else {
                    SupportState::Unknown
                };
                let component_id = outcomes
                    .first()
                    .map(|outcome| outcome.desired.target_id.clone())
                    .or_else(|| Some(format!("domain:{}", domain.as_str())));
                DomainCycleRecord {
                    domain: domain.as_str().to_string(),
                    pipeline_stage: PipelineStage::Complete,
                    selected_mode: decision.effective_config.mode_for(*domain),
                    support,
                    component_id,
                    action_outcomes: outcomes,
                }
            })
            .collect();
        let recovery_state = if restore_outcomes.iter().any(|outcome| {
            matches!(
                outcome.pending_restore,
                RestoreState::Pending | RestoreState::Failed
            )
        }) {
            "pending_restore"
        } else {
            "known_current_cycle"
        }
        .to_string();
        Self {
            schema_version: ENVELOPE_SCHEMA_VERSION,
            correlation_id,
            cycle_timestamp: snapshot.timestamp,
            pipeline_stage: PipelineStage::Complete,
            boot: BootEnvelope::from_boot_state(boot),
            observation: ObservationEnvelope::from_snapshot(snapshot),
            decision: DecisionEnvelope::from_decision(decision),
            domains,
            restore_outcomes,
            recovery_state,
        }
    }

    pub(crate) fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub(crate) fn to_json_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self).map(|mut line| {
            line.push('\n');
            line
        })
    }

    pub(crate) fn validate_schema(&self) -> Result<(), String> {
        if self.schema_version != ENVELOPE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported optid cycle schema version {}; expected {}",
                self.schema_version, ENVELOPE_SCHEMA_VERSION
            ));
        }
        if self.correlation_id.is_empty() {
            return Err("missing correlation_id".to_string());
        }
        let public_json = serde_json::to_string(self)
            .map_err(|error| format!("failed to validate public envelope: {error}"))?;
        for prohibited in ["/sys/", "/proc/", "/dev/", "/home/", "/root/", ".rs:"] {
            if public_json.contains(prohibited) {
                return Err(format!(
                    "public envelope contains prohibited diagnostic data: {prohibited}"
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn readback_from_result(
    expected: &str,
    result: std::io::Result<String>,
) -> ReadbackOutcome {
    match result {
        Ok(actual) => {
            let actual = actual.trim().to_string();
            if actual == expected {
                ReadbackOutcome::Confirmed { value: actual }
            } else {
                ReadbackOutcome::Mismatch {
                    expected: expected.to_string(),
                    actual,
                }
            }
        }
        Err(_) => ReadbackOutcome::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f3_correlation_ids_are_deterministic_and_distinct() {
        let mut ids = CycleIdGenerator::new(42);
        assert_eq!(ids.next(), "optid-000000000000002a-0000000000000000");
        assert_eq!(ids.next(), "optid-000000000000002a-0000000000000001");
    }

    #[test]
    fn f3_unknown_fields_are_tolerated() {
        let json = r#"{
            "schema_version": 2,
            "correlation_id": "cycle",
            "cycle_timestamp": 1,
            "pipeline_stage": "complete",
            "boot": {"policy_load_state":"ok","allowlist_load_state":"ok","allowlist_gate_enabled":false,"apply_armed":false,"baseline_armed":false},
            "observation": {"values":[]},
            "decision": {"selected_mode":"balanced","workload_class":"idle","workload_reason":"test","reasons":[],"contract":{"workload_class":"idle","cpu_wakeup_latency_us":null,"device_resume_latency_us":null}},
            "domains": [],
            "restore_outcomes": [],
            "recovery_state": "known_current_cycle",
            "future_field": {"ignored": true}
        }"#;
        let parsed: ControlCycleEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.correlation_id, "cycle");
    }

    #[test]
    fn f3_missing_optional_fields_use_defaults() {
        let json = r#"{
            "schema_version": 2,
            "correlation_id": "cycle",
            "cycle_timestamp": 1,
            "pipeline_stage": "complete",
            "boot": {"policy_load_state":"ok","allowlist_load_state":"ok","allowlist_gate_enabled":false,"apply_armed":false,"baseline_armed":false},
            "observation": {"values":[]},
            "decision": {"selected_mode":"balanced","workload_class":"idle","workload_reason":"test","reasons":[],"contract":{"workload_class":"idle","cpu_wakeup_latency_us":null,"device_resume_latency_us":null}},
            "domains": [],
            "recovery_state": "known_current_cycle"
        }"#;
        let parsed: ControlCycleEnvelope = serde_json::from_str(json).unwrap();
        assert!(parsed.restore_outcomes.is_empty());
    }

    #[test]
    fn f3_future_enum_values_fail_clearly() {
        let err = serde_json::from_str::<PipelineStage>("\"future_stage\"").unwrap_err();
        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn f3_outcomes_remain_machine_distinct() {
        assert_ne!(WriteOutcome::Unsupported, WriteOutcome::Denied);
        assert_ne!(WriteOutcome::Denied, WriteOutcome::Skipped);
        assert_ne!(
            WriteOutcome::Skipped,
            WriteOutcome::Failed {
                error_kind: ErrorKindCode::Other
            }
        );
        assert_ne!(
            WriteOutcome::Restored,
            WriteOutcome::RestorationFailed {
                error_kind: ErrorKindCode::Other
            }
        );
        assert_ne!(
            WriteOutcome::OwnershipRelinquished,
            WriteOutcome::NotApplicable
        );
    }

    #[test]
    fn f3_readback_detects_mismatch() {
        assert_eq!(
            readback_from_result("42", Ok("43\n".to_string())),
            ReadbackOutcome::Mismatch {
                expected: "42".to_string(),
                actual: "43".to_string(),
            }
        );
    }

    #[test]
    fn f3_public_action_identity_does_not_leak_raw_path() {
        let action = Action::VmSysctl {
            path: "/proc/sys/vm/swappiness".into(),
            value: "42".to_string(),
            reason: "test".to_string(),
        };
        let json = serde_json::to_string(&DesiredAction::from_action(&action)).unwrap();
        assert!(!json.contains("/proc/"));
        assert!(!json.contains("/home/"));
        assert!(!json.contains(".rs:"));
        assert!(json.contains("vm-sysctl:swappiness"));
    }
}
