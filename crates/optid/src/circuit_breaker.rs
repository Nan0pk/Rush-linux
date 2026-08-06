//! S5D — persistent per-domain circuit breakers and controlled canary re-entry.
//!
//! Circuit state is evaluated before desired-state reconciliation. A scoped
//! circuit is keyed by domain, operation, stable target identity, system
//! firmware identity, and failure class. Repeated operational failures open the
//! circuit, remove that domain from the desired set, persist quarantine across
//! restart, and allow one monitored canary only after both cooldown and a
//! successful observe-only recovery cycle.
//!
//! Unknown process-wide corruption opens a global circuit. Global state never
//! closes automatically; root-authorized intervention is required.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::envelope::{
    ActionOutcome, GateDisposition, GateEvaluation, GateReasonCode, GateStage, OutcomeReasonCode,
    OwnershipState, PipelineStage, ReadbackOutcome, RestoreOutcome, TargetOutcome, WriteOutcome,
};
use crate::kernel_io::KernelRead;

const CIRCUIT_SCHEMA_VERSION: u32 = 1;
pub(crate) const DEFAULT_FAILURE_THRESHOLD: u32 = 2;
pub(crate) const DEFAULT_COOLDOWN_SECS: u64 = 300;
const MIN_FAILURE_THRESHOLD: u32 = 2;
const MAX_FAILURE_THRESHOLD: u32 = 10;
const MIN_COOLDOWN_SECS: u64 = 30;
const MAX_COOLDOWN_SECS: u64 = 86_400;
const PRODUCTION_STATE_DIR: &str = "/run/optid";
const PERSISTENT_CIRCUIT_FILE: &str = "/var/lib/optid/recovery/circuits-v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CircuitConfig {
    failure_threshold: u32,
    cooldown_secs: u64,
}

impl CircuitConfig {
    pub(crate) fn validated(failure_threshold: u32, cooldown_secs: u64) -> io::Result<Self> {
        if !(MIN_FAILURE_THRESHOLD..=MAX_FAILURE_THRESHOLD).contains(&failure_threshold) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "S5D failure threshold must be {MIN_FAILURE_THRESHOLD}..={MAX_FAILURE_THRESHOLD}"
                ),
            ));
        }
        if !(MIN_COOLDOWN_SECS..=MAX_COOLDOWN_SECS).contains(&cooldown_secs) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("S5D cooldown must be {MIN_COOLDOWN_SECS}..={MAX_COOLDOWN_SECS} seconds"),
            ));
        }
        Ok(Self {
            failure_threshold,
            cooldown_secs,
        })
    }

    fn safe_default() -> Self {
        Self {
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            cooldown_secs: DEFAULT_COOLDOWN_SECS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CircuitScope {
    pub(crate) domain: String,
    pub(crate) operation: String,
    pub(crate) target_id: String,
    pub(crate) hardware_id: String,
    pub(crate) firmware_id: String,
}

impl CircuitScope {
    pub(crate) fn from_action(action: &Action, read: &dyn KernelRead) -> Self {
        Self {
            domain: action
                .domain()
                .map(|domain| domain.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            operation: action.desired_operation().to_string(),
            target_id: action.stable_target_id(),
            hardware_id: action_hardware_id(action, read),
            firmware_id: system_firmware_id(read),
        }
    }

    pub(crate) fn for_restore(
        domain: impl Into<String>,
        target_id: impl Into<String>,
        read: &dyn KernelRead,
    ) -> Self {
        let target_id = target_id.into();
        Self {
            domain: domain.into(),
            operation: "restore_owned_target".to_string(),
            hardware_id: target_id.clone(),
            target_id,
            firmware_id: system_firmware_id(read),
        }
    }

    fn id(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.domain, self.operation, self.target_id, self.hardware_id, self.firmware_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureClass {
    Journal,
    Write,
    Readback,
    Restore,
    Runtime,
}

impl FailureClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Journal => "journal",
            Self::Write => "write",
            Self::Readback => "readback",
            Self::Restore => "restore",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CircuitPermit {
    Normal,
    Canary,
    Suppressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircuitDecision {
    pub(crate) permit: CircuitPermit,
    pub(crate) detail: String,
}

impl CircuitDecision {
    fn normal() -> Self {
        Self {
            permit: CircuitPermit::Normal,
            detail: "S5D circuit closed".to_string(),
        }
    }

    fn canary() -> Self {
        Self {
            permit: CircuitPermit::Canary,
            detail: "S5D one-shot monitored canary admitted".to_string(),
        }
    }

    fn suppressed(detail: impl Into<String>) -> Self {
        Self {
            permit: CircuitPermit::Suppressed,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircuitTransition {
    pub(crate) opened: bool,
    pub(crate) closed: bool,
    pub(crate) detail: String,
}

impl CircuitTransition {
    fn unchanged(detail: impl Into<String>) -> Self {
        Self {
            opened: false,
            closed: false,
            detail: detail.into(),
        }
    }

    fn opened(detail: impl Into<String>) -> Self {
        Self {
            opened: true,
            closed: false,
            detail: detail.into(),
        }
    }

    fn closed(detail: impl Into<String>) -> Self {
        Self {
            opened: false,
            closed: true,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircuitRecord {
    scope: CircuitScope,
    failure_class: FailureClass,
    consecutive_failures: u32,
    open: bool,
    opened_at: u64,
    cooldown_until: u64,
    recovery_verified: bool,
    canary_in_flight: bool,
    last_failure_at: u64,
    last_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GlobalCircuit {
    opened_at: u64,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedCircuitState {
    schema_version: u32,
    last_seen_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    global: Option<GlobalCircuit>,
    #[serde(default)]
    records: BTreeMap<String, CircuitRecord>,
}

impl Default for PersistedCircuitState {
    fn default() -> Self {
        Self {
            schema_version: CIRCUIT_SCHEMA_VERSION,
            last_seen_at: 0,
            global: None,
            records: BTreeMap::new(),
        }
    }
}

pub(crate) struct CircuitBreaker {
    path: PathBuf,
    config: CircuitConfig,
    state: PersistedCircuitState,
    persistence_blocked: bool,
    startup_warning: Option<String>,
}

impl CircuitBreaker {
    pub(crate) fn load(path: PathBuf, failure_threshold: u32, cooldown_secs: u64) -> Self {
        let mut startup_warning = None;
        let config = match CircuitConfig::validated(failure_threshold, cooldown_secs) {
            Ok(config) => config,
            Err(error) => {
                startup_warning = Some(error.to_string());
                CircuitConfig::safe_default()
            }
        };

        let mut state = PersistedCircuitState::default();
        let mut persistence_blocked = false;
        match fs::read(&path) {
            Ok(bytes) => {
                let permissions_ok = fs::metadata(&path)
                    .map(|metadata| {
                        let private = metadata.permissions().mode() & 0o077 == 0;
                        let root_owned =
                            path != Path::new(PERSISTENT_CIRCUIT_FILE) || metadata.uid() == 0;
                        private && root_owned
                    })
                    .unwrap_or(false);
                match serde_json::from_slice::<PersistedCircuitState>(&bytes) {
                    Ok(parsed)
                        if parsed.schema_version == CIRCUIT_SCHEMA_VERSION && permissions_ok =>
                    {
                        state = parsed;
                    }
                    Ok(parsed) if parsed.schema_version != CIRCUIT_SCHEMA_VERSION => {
                        let reason = format!(
                            "unsupported S5D circuit schema {}; expected {}",
                            parsed.schema_version, CIRCUIT_SCHEMA_VERSION
                        );
                        state.global = Some(GlobalCircuit {
                            opened_at: state.last_seen_at,
                            reason: reason.clone(),
                        });
                        startup_warning = Some(reason);
                        persistence_blocked = true;
                    }
                    Ok(_) => {
                        let reason =
                            "S5D circuit state is not root-private; all actuation observe-only"
                                .to_string();
                        state.global = Some(GlobalCircuit {
                            opened_at: state.last_seen_at,
                            reason: reason.clone(),
                        });
                        startup_warning = Some(reason);
                        persistence_blocked = true;
                    }
                    Err(error) => {
                        let reason = format!(
                            "malformed S5D circuit state preserved for inspection: {error}"
                        );
                        state.global = Some(GlobalCircuit {
                            opened_at: state.last_seen_at,
                            reason: reason.clone(),
                        });
                        startup_warning = Some(reason);
                        persistence_blocked = true;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let reason = format!("cannot read S5D circuit state: {error}");
                state.global = Some(GlobalCircuit {
                    opened_at: 0,
                    reason: reason.clone(),
                });
                startup_warning = Some(reason);
                persistence_blocked = true;
            }
        }

        if let Some(reason) = startup_warning.clone() {
            state.global.get_or_insert(GlobalCircuit {
                opened_at: state.last_seen_at,
                reason,
            });
        }

        Self {
            path,
            config,
            state,
            persistence_blocked,
            startup_warning,
        }
    }

    pub(crate) fn state_path_for(runtime_state_dir: &Path) -> PathBuf {
        if runtime_state_dir == Path::new(PRODUCTION_STATE_DIR) {
            PathBuf::from(PERSISTENT_CIRCUIT_FILE)
        } else {
            runtime_state_dir.join("persistent-circuits-v1.json")
        }
    }

    pub(crate) fn startup_warning(&self) -> Option<&str> {
        self.startup_warning.as_deref()
    }

    pub(crate) fn is_global_open(&self) -> bool {
        self.state.global.is_some()
    }

    pub(crate) fn decide(&mut self, scope: &CircuitScope, now: u64) -> io::Result<CircuitDecision> {
        if let Some(global) = &self.state.global {
            return Ok(CircuitDecision::suppressed(format!(
                "S5D global observe-only circuit open: {}",
                global.reason
            )));
        }

        let rollback = now < self.state.last_seen_at;
        self.state.last_seen_at = self.state.last_seen_at.max(now);

        let matching = self.matching_record_keys(scope);
        if matching.is_empty() {
            return Ok(CircuitDecision::normal());
        }

        if rollback {
            return Ok(CircuitDecision::suppressed(
                "S5D clock moved backwards; cooldown cannot be shortened",
            ));
        }

        let records: Vec<&CircuitRecord> = matching
            .iter()
            .filter_map(|key| self.state.records.get(key))
            .filter(|record| record.open)
            .collect();
        if records.is_empty() {
            return Ok(CircuitDecision::normal());
        }
        if records.iter().any(|record| record.canary_in_flight) {
            return Ok(CircuitDecision::suppressed(
                "S5D monitored canary already consumed; awaiting verified outcome",
            ));
        }
        if records.iter().any(|record| !record.recovery_verified) {
            return Ok(CircuitDecision::suppressed(
                "S5D circuit open; successful observe-only recovery cycle required",
            ));
        }
        let cooldown_until = records
            .iter()
            .map(|record| record.cooldown_until)
            .max()
            .unwrap_or(now);
        if now < cooldown_until {
            return Ok(CircuitDecision::suppressed(format!(
                "S5D circuit open until {cooldown_until}; domain remains observe-only"
            )));
        }

        for key in matching {
            if let Some(record) = self.state.records.get_mut(&key) {
                if record.open {
                    record.canary_in_flight = true;
                }
            }
        }
        self.persist()?;
        Ok(CircuitDecision::canary())
    }

    pub(crate) fn observe_outcome(
        &mut self,
        scope: &CircuitScope,
        permit: CircuitPermit,
        outcome: &ActionOutcome,
        now: u64,
    ) -> io::Result<CircuitTransition> {
        if let Some((class, reason)) = classify_failure(outcome) {
            return self.record_failure(scope, class, reason, permit, now);
        }
        if outcome_is_verified_success(outcome) {
            return self.record_success(scope, permit);
        }
        Ok(CircuitTransition::unchanged(
            "S5D outcome was neither a verified success nor a counted failure",
        ))
    }

    pub(crate) fn observe_restore_outcome(
        &mut self,
        scope: &CircuitScope,
        outcome: &RestoreOutcome,
        now: u64,
    ) -> io::Result<CircuitTransition> {
        if matches!(
            outcome.write_outcome,
            WriteOutcome::RestorationFailed { .. }
        ) || outcome.reason == OutcomeReasonCode::RestoreFailed
        {
            return self.record_failure(
                scope,
                FailureClass::Restore,
                outcome
                    .detail
                    .clone()
                    .unwrap_or_else(|| "owned-target restoration failed".to_string()),
                CircuitPermit::Normal,
                now,
            );
        }
        Ok(CircuitTransition::unchanged(
            "S5D restore outcome did not count as a failure",
        ))
    }

    pub(crate) fn record_runtime_error(
        &mut self,
        scope: &CircuitScope,
        permit: CircuitPermit,
        error: &io::Error,
        now: u64,
    ) -> io::Result<CircuitTransition> {
        self.record_failure(
            scope,
            FailureClass::Runtime,
            format!("runtime error: {:?}", error.kind()),
            permit,
            now,
        )
    }

    pub(crate) fn mark_recovery_success(&mut self, now: u64) -> io::Result<()> {
        self.state.last_seen_at = self.state.last_seen_at.max(now);
        let mut changed = false;
        for record in self.state.records.values_mut() {
            if record.open && !record.recovery_verified {
                record.recovery_verified = true;
                changed = true;
            }
        }
        if changed {
            self.persist()?;
        }
        Ok(())
    }

    pub(crate) fn trip_global(&mut self, reason: impl Into<String>, now: u64) -> io::Result<()> {
        self.state.last_seen_at = self.state.last_seen_at.max(now);
        self.state.global = Some(GlobalCircuit {
            opened_at: now,
            reason: reason.into(),
        });
        self.persist()
    }

    pub(crate) fn clear_domain(&mut self, domain: &str, effective_uid: u32) -> io::Result<usize> {
        self.require_clear_authorization(effective_uid)?;
        let keys: Vec<String> = self
            .state
            .records
            .iter()
            .filter(|(_, record)| record.scope.domain == domain)
            .map(|(key, _)| key.clone())
            .collect();
        let removed = keys.len();
        for key in keys {
            self.state.records.remove(&key);
        }
        self.persist()?;
        Ok(removed)
    }

    pub(crate) fn clear_all(&mut self, effective_uid: u32) -> io::Result<usize> {
        self.require_clear_authorization(effective_uid)?;
        let removed = self.state.records.len() + usize::from(self.state.global.is_some());
        self.state.records.clear();
        self.state.global = None;
        self.persist()?;
        Ok(removed)
    }

    fn require_clear_authorization(&self, effective_uid: u32) -> io::Result<()> {
        if effective_uid != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "S5D circuit clear requires effective UID 0",
            ));
        }
        if self.persistence_blocked || self.startup_warning.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "S5D circuit state or configuration is invalid; preserve state evidence and repair the file/config explicitly before clearing",
            ));
        }
        Ok(())
    }

    pub(crate) fn public_json(&self) -> io::Result<String> {
        #[derive(Serialize)]
        struct PublicRecord<'a> {
            domain: &'a str,
            operation: &'a str,
            target_id: &'a str,
            hardware_id: &'a str,
            firmware_id: &'a str,
            failure_class: FailureClass,
            consecutive_failures: u32,
            state: &'static str,
            cooldown_until: u64,
            recovery_verified: bool,
            last_reason: &'a str,
        }
        #[derive(Serialize)]
        struct PublicState<'a> {
            schema_version: u32,
            global_observe_only: bool,
            global_reason: Option<&'a str>,
            records: Vec<PublicRecord<'a>>,
        }

        let records = self
            .state
            .records
            .values()
            .map(|record| PublicRecord {
                domain: &record.scope.domain,
                operation: &record.scope.operation,
                target_id: &record.scope.target_id,
                hardware_id: &record.scope.hardware_id,
                firmware_id: &record.scope.firmware_id,
                failure_class: record.failure_class,
                consecutive_failures: record.consecutive_failures,
                state: if record.canary_in_flight {
                    "canary"
                } else if record.open {
                    "open"
                } else {
                    "closed"
                },
                cooldown_until: record.cooldown_until,
                recovery_verified: record.recovery_verified,
                last_reason: &record.last_reason,
            })
            .collect();
        serde_json::to_string_pretty(&PublicState {
            schema_version: CIRCUIT_SCHEMA_VERSION,
            global_observe_only: self.state.global.is_some(),
            global_reason: self
                .state
                .global
                .as_ref()
                .map(|global| global.reason.as_str()),
            records,
        })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(crate) fn summary(&self) -> String {
        let open_scopes = self
            .state
            .records
            .values()
            .filter(|record| record.open)
            .map(|record| record.scope.domain.as_str())
            .collect::<BTreeSet<_>>();
        format!(
            "global_open={} open_domains={} records={}",
            self.state.global.is_some(),
            if open_scopes.is_empty() {
                "none".to_string()
            } else {
                open_scopes.into_iter().collect::<Vec<_>>().join(",")
            },
            self.state.records.len()
        )
    }

    fn record_failure(
        &mut self,
        scope: &CircuitScope,
        class: FailureClass,
        reason: String,
        permit: CircuitPermit,
        now: u64,
    ) -> io::Result<CircuitTransition> {
        self.state.last_seen_at = self.state.last_seen_at.max(now);
        if permit == CircuitPermit::Canary {
            for key in self.matching_record_keys(scope) {
                if let Some(existing) = self.state.records.get_mut(&key) {
                    existing.open = true;
                    existing.opened_at = now;
                    existing.cooldown_until = now.saturating_add(self.config.cooldown_secs);
                    existing.recovery_verified = false;
                    existing.canary_in_flight = false;
                }
            }
        }
        let key = record_key(scope, class);
        let record = self
            .state
            .records
            .entry(key)
            .or_insert_with(|| CircuitRecord {
                scope: scope.clone(),
                failure_class: class,
                consecutive_failures: 0,
                open: false,
                opened_at: 0,
                cooldown_until: 0,
                recovery_verified: false,
                canary_in_flight: false,
                last_failure_at: 0,
                last_reason: String::new(),
            });

        record.last_failure_at = now;
        record.last_reason = reason.clone();
        record.consecutive_failures = record.consecutive_failures.saturating_add(1);
        let must_open = permit == CircuitPermit::Canary
            || record.open
            || record.consecutive_failures >= self.config.failure_threshold;
        if must_open {
            record.open = true;
            record.opened_at = now;
            record.cooldown_until = now.saturating_add(self.config.cooldown_secs);
            record.recovery_verified = false;
            record.canary_in_flight = false;
            if permit == CircuitPermit::Canary {
                record.consecutive_failures = record
                    .consecutive_failures
                    .max(self.config.failure_threshold);
            }
        }
        let consecutive_failures = record.consecutive_failures;
        self.persist()?;

        if must_open {
            Ok(CircuitTransition::opened(format!(
                "S5D opened {} circuit for domain={} operation={} target={} hwid={} firmware={} after {} failure(s): {}",
                class.as_str(),
                scope.domain,
                scope.operation,
                scope.target_id,
                scope.hardware_id,
                scope.firmware_id,
                consecutive_failures,
                reason,
            )))
        } else {
            Ok(CircuitTransition::unchanged(format!(
                "S5D recorded {} failure {}/{} for domain={} operation={}: {}",
                class.as_str(),
                consecutive_failures,
                self.config.failure_threshold,
                scope.domain,
                scope.operation,
                reason,
            )))
        }
    }

    fn record_success(
        &mut self,
        scope: &CircuitScope,
        permit: CircuitPermit,
    ) -> io::Result<CircuitTransition> {
        let keys = self.matching_record_keys(scope);
        if permit == CircuitPermit::Canary {
            for key in keys {
                self.state.records.remove(&key);
            }
            self.persist()?;
            return Ok(CircuitTransition::closed(format!(
                "S5D verified canary closed circuit for domain={} operation={} target={} hwid={} firmware={}",
                scope.domain,
                scope.operation,
                scope.target_id,
                scope.hardware_id,
                scope.firmware_id
            )));
        }

        let mut changed = false;
        for key in keys {
            if self
                .state
                .records
                .get(&key)
                .is_some_and(|record| !record.open)
            {
                self.state.records.remove(&key);
                changed = true;
            }
        }
        if changed {
            self.persist()?;
        }
        Ok(CircuitTransition::unchanged(
            "S5D verified success reset scoped pre-threshold failures",
        ))
    }

    fn matching_record_keys(&self, scope: &CircuitScope) -> Vec<String> {
        self.state
            .records
            .iter()
            .filter(|(_, record)| record.scope == *scope)
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn persist(&mut self) -> io::Result<()> {
        if self.persistence_blocked {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "S5D circuit persistence is blocked to preserve invalid state evidence",
            ));
        }
        let Some(parent) = self.path.parent() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "S5D circuit state path has no parent",
            ));
        };
        fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(&self.state)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = parent.join(format!(
            ".circuits-v1-{}-{}.tmp",
            std::process::id(),
            self.state.last_seen_at
        ));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        fs::rename(&temporary, &self.path)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        File::open(parent)?.sync_all()
    }
}

pub(crate) fn circuit_suppressed_outcome(action: &Action, detail: &str) -> ActionOutcome {
    let mut outcome = ActionOutcome::new(action);
    outcome.gates.push(GateEvaluation::allowed(
        GateStage::DomainMode,
        GateReasonCode::DomainActuate,
    ));
    outcome.gates.push(GateEvaluation::denied(
        GateStage::ApplyArmed,
        GateReasonCode::ApplyDisarmedByBootState,
        detail,
    ));
    outcome.targets.push(TargetOutcome::denied(
        action.stable_target_id(),
        PipelineStage::ApplyGate,
        detail.to_string(),
    ));
    outcome
}

pub(crate) fn circuit_runtime_failure_outcome(action: &Action, error: &io::Error) -> ActionOutcome {
    let mut outcome = ActionOutcome::new(action);
    outcome.gates.push(GateEvaluation::allowed(
        GateStage::DomainMode,
        GateReasonCode::DomainActuate,
    ));
    outcome.gates.push(GateEvaluation::allowed(
        GateStage::ApplyArmed,
        GateReasonCode::ApplyArmed,
    ));
    outcome
        .targets
        .push(TargetOutcome::failed(action.stable_target_id(), error));
    outcome
}

fn classify_failure(outcome: &ActionOutcome) -> Option<(FailureClass, String)> {
    if outcome.gates.iter().any(|gate| {
        gate.stage == GateStage::RecoveryJournal && gate.disposition == GateDisposition::Denied
    }) {
        return Some((
            FailureClass::Journal,
            "persistent transaction or recovery journal gate failed".to_string(),
        ));
    }

    for target in &outcome.targets {
        if target.reason == OutcomeReasonCode::ReadbackMismatch
            || matches!(target.readback, ReadbackOutcome::Mismatch { .. })
        {
            return Some((
                FailureClass::Readback,
                target
                    .detail
                    .clone()
                    .unwrap_or_else(|| "readback mismatch".to_string()),
            ));
        }
        match target.write_outcome {
            WriteOutcome::Failed { .. } => {
                return Some((
                    FailureClass::Write,
                    target
                        .detail
                        .clone()
                        .unwrap_or_else(|| "kernel write failed".to_string()),
                ));
            }
            WriteOutcome::RestorationFailed { .. } => {
                return Some((
                    FailureClass::Restore,
                    target
                        .detail
                        .clone()
                        .unwrap_or_else(|| "handback failed".to_string()),
                ));
            }
            _ => {}
        }
    }
    None
}

fn outcome_is_verified_success(outcome: &ActionOutcome) -> bool {
    !outcome.targets.is_empty()
        && outcome.targets.iter().all(|target| {
            target.ownership == OwnershipState::Optid
                && matches!(target.readback, ReadbackOutcome::Confirmed { .. })
                && matches!(
                    target.write_outcome,
                    WriteOutcome::Applied | WriteOutcome::Redundant
                )
        })
}

fn record_key(scope: &CircuitScope, class: FailureClass) -> String {
    format!("{}|{}", scope.id(), class.as_str())
}

fn action_hardware_id(action: &Action, read: &dyn KernelRead) -> String {
    let discovered = match action {
        Action::DeviceResumeLatency { path, .. } => path
            .parent()
            .and_then(Path::parent)
            .and_then(|device| read_modalias(read, device)),
        Action::RuntimePm { device_dir, .. } | Action::PcieAspm { device_dir, .. } => {
            read_modalias(read, device_dir)
        }
        Action::SataAlpm { host_dir, .. }
        | Action::Backlight {
            device_dir: host_dir,
            ..
        } => read_modalias_from_ancestors(read, host_dir),
        Action::CpuEpp { .. }
        | Action::PlatformProfile { .. }
        | Action::SystemdSetProperty { .. }
        | Action::VmSysctl { .. }
        | Action::CpuDmaLatency { .. } => None,
    };
    discovered.unwrap_or_else(|| system_hardware_id(read))
}

fn read_modalias(read: &dyn KernelRead, device_dir: &Path) -> Option<String> {
    let raw = read.read_to_string(&device_dir.join("modalias")).ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| sanitize_identity_token(trimmed))
}

fn read_modalias_from_ancestors(read: &dyn KernelRead, start: &Path) -> Option<String> {
    let canonical = read
        .canonicalize(start)
        .unwrap_or_else(|_| start.to_path_buf());
    canonical
        .ancestors()
        .find_map(|ancestor| read_modalias(read, ancestor))
}

fn system_hardware_id(read: &dyn KernelRead) -> String {
    let mut values = Vec::new();
    for path in [
        "/sys/class/dmi/id/sys_vendor",
        "/sys/class/dmi/id/product_name",
        "/sys/class/dmi/id/product_version",
        "/sys/class/dmi/id/board_name",
    ] {
        if let Ok(value) = read.read_to_string(Path::new(path)) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                values.push(trimmed.to_string());
            }
        }
    }
    if values.is_empty() {
        return "hwid:unreported".to_string();
    }
    format!("hwid:{:016x}", stable_fnv1a(values.join("|").as_bytes()))
}

fn sanitize_identity_token(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ':' | '_' | '-' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn system_firmware_id(read: &dyn KernelRead) -> String {
    let mut values = Vec::new();
    for path in [
        "/sys/class/dmi/id/bios_vendor",
        "/sys/class/dmi/id/bios_version",
        "/sys/class/dmi/id/bios_release",
        "/sys/class/dmi/id/board_version",
    ] {
        if let Ok(value) = read.read_to_string(Path::new(path)) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                values.push(trimmed.to_string());
            }
        }
    }
    if values.is_empty() {
        return "firmware:unreported".to_string();
    }
    format!(
        "firmware:{:016x}",
        stable_fnv1a(values.join("|").as_bytes())
    )
}

fn stable_fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CircuitClearRequest {
    Domain(String),
    All,
}

pub(crate) fn extract_circuit_clear_request<I>(
    args: I,
) -> Result<(Option<CircuitClearRequest>, Vec<String>), String>
where
    I: IntoIterator<Item = String>,
{
    let mut request = None;
    let mut remaining = Vec::new();
    let mut iter = args.into_iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--clear-all-circuits" => {
                if request.is_some() {
                    return Err(
                        "--clear-all-circuits and --clear-circuit-domain are mutually exclusive"
                            .to_string(),
                    );
                }
                request = Some(CircuitClearRequest::All);
            }
            "--clear-circuit-domain" => {
                let domain = iter
                    .next()
                    .ok_or_else(|| "--clear-circuit-domain requires a domain name".to_string())?;
                if request.is_some() {
                    return Err(
                        "--clear-all-circuits and --clear-circuit-domain are mutually exclusive"
                            .to_string(),
                    );
                }
                request = Some(CircuitClearRequest::Domain(domain));
            }
            _ => remaining.push(argument),
        }
    }
    Ok((request, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{ReadbackOutcome, TargetOutcome};
    use crate::kernel_io::MemoryKernel;

    fn temp_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("optid-s5d-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create S5D test directory");
        path.join("circuits.json")
    }

    fn scope(domain: &str, firmware: &str) -> CircuitScope {
        CircuitScope {
            domain: domain.to_string(),
            operation: "set_value".to_string(),
            target_id: format!("{domain}:target"),
            hardware_id: format!("hwid:{domain}"),
            firmware_id: firmware.to_string(),
        }
    }

    fn breaker(name: &str) -> CircuitBreaker {
        CircuitBreaker::load(
            temp_path(name),
            DEFAULT_FAILURE_THRESHOLD,
            DEFAULT_COOLDOWN_SECS,
        )
    }

    #[test]
    fn s5d_clear_commands_are_one_shot_and_mutually_exclusive() {
        let (request, remaining) = extract_circuit_clear_request([
            "--state-dir".to_string(),
            "/tmp/optid".to_string(),
            "--clear-circuit-domain".to_string(),
            "runtime_pm".to_string(),
        ])
        .expect("extract domain clear");
        assert_eq!(
            request,
            Some(CircuitClearRequest::Domain("runtime_pm".to_string()))
        );
        assert_eq!(remaining, ["--state-dir", "/tmp/optid"]);

        let (request, remaining) =
            extract_circuit_clear_request(["--clear-all-circuits".to_string()])
                .expect("extract global clear");
        assert_eq!(request, Some(CircuitClearRequest::All));
        assert!(remaining.is_empty());

        let conflict = extract_circuit_clear_request([
            "--clear-all-circuits".to_string(),
            "--clear-circuit-domain".to_string(),
            "runtime_pm".to_string(),
        ])
        .expect_err("clear forms must conflict");
        assert!(conflict.contains("mutually exclusive"));

        let missing = extract_circuit_clear_request(["--clear-circuit-domain".to_string()])
            .expect_err("domain value is required");
        assert!(missing.contains("requires a domain name"));
    }

    fn verified_outcome() -> ActionOutcome {
        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test".to_string(),
        );
        let mut outcome = ActionOutcome::new(&action);
        outcome.targets.push(TargetOutcome::applied(
            action.stable_target_id(),
            "60",
            ReadbackOutcome::Confirmed {
                value: "60".to_string(),
            },
        ));
        outcome
    }

    #[test]
    fn s5d_repeated_failure_opens_persistent_scope() {
        let mut breaker = breaker("threshold");
        let scope = scope("runtime_pm", "fw-a");
        let first = breaker
            .record_failure(
                &scope,
                FailureClass::Write,
                "first".to_string(),
                CircuitPermit::Normal,
                10,
            )
            .expect("record first failure");
        assert!(!first.opened);
        let second = breaker
            .record_failure(
                &scope,
                FailureClass::Write,
                "second".to_string(),
                CircuitPermit::Normal,
                11,
            )
            .expect("record second failure");
        assert!(second.opened);
        assert_eq!(
            breaker.decide(&scope, 12).expect("decide").permit,
            CircuitPermit::Suppressed
        );
    }

    #[test]
    fn s5d_restart_preserves_open_circuit() {
        let path = temp_path("restart");
        let scope = scope("backlight", "fw-a");
        let mut first = CircuitBreaker::load(
            path.clone(),
            DEFAULT_FAILURE_THRESHOLD,
            DEFAULT_COOLDOWN_SECS,
        );
        for now in [10, 11] {
            first
                .record_failure(
                    &scope,
                    FailureClass::Readback,
                    "mismatch".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("persist failure");
        }
        let mut restarted =
            CircuitBreaker::load(path, DEFAULT_FAILURE_THRESHOLD, DEFAULT_COOLDOWN_SECS);
        assert_eq!(
            restarted.decide(&scope, 500).expect("decide").permit,
            CircuitPermit::Suppressed
        );
    }

    #[test]
    fn s5d_cooldown_requires_recovery_before_one_canary() {
        let mut breaker = breaker("cooldown");
        let scope = scope("runtime_pm", "fw-a");
        for now in [10, 11] {
            breaker
                .record_failure(
                    &scope,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        assert_eq!(
            breaker
                .decide(&scope, 11 + DEFAULT_COOLDOWN_SECS)
                .expect("decide")
                .permit,
            CircuitPermit::Suppressed
        );
        breaker
            .mark_recovery_success(11 + DEFAULT_COOLDOWN_SECS)
            .expect("mark recovery");
        assert_eq!(
            breaker
                .decide(&scope, 11 + DEFAULT_COOLDOWN_SECS)
                .expect("decide")
                .permit,
            CircuitPermit::Canary
        );
        assert_eq!(
            breaker
                .decide(&scope, 11 + DEFAULT_COOLDOWN_SECS + 1)
                .expect("decide")
                .permit,
            CircuitPermit::Suppressed
        );
    }

    #[test]
    fn s5d_canary_success_closes_circuit() {
        let mut breaker = breaker("canary-success");
        let scope = scope("runtime_pm", "fw-a");
        for now in [10, 11] {
            breaker
                .record_failure(
                    &scope,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        breaker.mark_recovery_success(400).expect("recovery");
        let permit = breaker.decide(&scope, 400).expect("canary").permit;
        assert_eq!(permit, CircuitPermit::Canary);
        let transition = breaker
            .observe_outcome(&scope, permit, &verified_outcome(), 400)
            .expect("record success");
        assert!(transition.closed);
        assert_eq!(
            breaker.decide(&scope, 401).expect("decide").permit,
            CircuitPermit::Normal
        );
    }

    #[test]
    fn s5d_canary_failure_reopens_immediately() {
        let mut breaker = breaker("canary-failure");
        let scope = scope("runtime_pm", "fw-a");
        for now in [10, 11] {
            breaker
                .record_failure(
                    &scope,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        breaker.mark_recovery_success(400).expect("recovery");
        let permit = breaker.decide(&scope, 400).expect("canary").permit;
        let transition = breaker
            .record_failure(
                &scope,
                FailureClass::Readback,
                "canary mismatch".to_string(),
                permit,
                400,
            )
            .expect("record canary failure");
        assert!(transition.opened);
        assert_eq!(
            breaker.decide(&scope, 401).expect("decide").permit,
            CircuitPermit::Suppressed
        );
        breaker
            .mark_recovery_success(800)
            .expect("recovery after canary failure");
        assert_eq!(
            breaker.decide(&scope, 800).expect("second canary").permit,
            CircuitPermit::Canary
        );
    }

    #[test]
    fn s5d_firmware_change_uses_independent_scope() {
        let mut breaker = breaker("firmware");
        let old = scope("runtime_pm", "fw-a");
        let new = scope("runtime_pm", "fw-b");
        for now in [10, 11] {
            breaker
                .record_failure(
                    &old,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        assert_eq!(
            breaker.decide(&old, 12).expect("old").permit,
            CircuitPermit::Suppressed
        );
        assert_eq!(
            breaker.decide(&new, 12).expect("new").permit,
            CircuitPermit::Normal
        );
    }

    #[test]
    fn s5d_manual_clear_requires_root_authorization() {
        let mut breaker = breaker("clear");
        let scope = scope("runtime_pm", "fw-a");
        for now in [10, 11] {
            breaker
                .record_failure(
                    &scope,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        assert_eq!(
            breaker
                .clear_domain(&scope.domain, 1000)
                .expect_err("non-root clear must fail")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            breaker.clear_domain(&scope.domain, 0).expect("root clear"),
            1
        );
        assert_eq!(
            breaker.decide(&scope, 12).expect("decide").permit,
            CircuitPermit::Normal
        );
    }

    #[test]
    fn s5d_backward_clock_jump_never_shortens_cooldown() {
        let mut breaker = breaker("clock");
        let scope = scope("runtime_pm", "fw-a");
        for now in [1_000, 1_001] {
            breaker
                .record_failure(
                    &scope,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        breaker.mark_recovery_success(1_100).expect("recovery");
        assert_eq!(
            breaker.decide(&scope, 900).expect("rollback").permit,
            CircuitPermit::Suppressed
        );
        assert_eq!(
            breaker.decide(&scope, 1_200).expect("cooldown").permit,
            CircuitPermit::Suppressed
        );
    }

    #[test]
    fn s5d_multi_domain_failure_isolation() {
        let mut breaker = breaker("isolation");
        let failed = scope("runtime_pm", "fw-a");
        let healthy = scope("backlight", "fw-a");
        for now in [10, 11] {
            breaker
                .record_failure(
                    &failed,
                    FailureClass::Write,
                    "failure".to_string(),
                    CircuitPermit::Normal,
                    now,
                )
                .expect("record");
        }
        assert_eq!(
            breaker.decide(&failed, 12).expect("failed").permit,
            CircuitPermit::Suppressed
        );
        assert_eq!(
            breaker.decide(&healthy, 12).expect("healthy").permit,
            CircuitPermit::Normal
        );
    }

    #[test]
    fn s5d_restore_failures_open_only_the_affected_domain() {
        let mut breaker = breaker("restore-failure");
        let scope = CircuitScope::for_restore("runtime_pm", "runtime-pm:abc", &MemoryKernel::new());
        let outcome = RestoreOutcome {
            target_id: "runtime-pm:abc".to_string(),
            pipeline_stage: PipelineStage::Restore,
            reason: OutcomeReasonCode::RestoreFailed,
            write_attempted: true,
            write_outcome: WriteOutcome::RestorationFailed {
                error_kind: crate::envelope::ErrorKindCode::Other,
            },
            readback: ReadbackOutcome::Unavailable,
            ownership: OwnershipState::Optid,
            pending_restore: crate::envelope::RestoreState::Pending,
            responsible_subsystem: crate::envelope::ResponsibleSubsystem::Restoration,
            detail: Some("restore write failed".to_string()),
        };
        let first = breaker
            .observe_restore_outcome(&scope, &outcome, 10)
            .expect("first restore failure");
        assert!(!first.opened);
        let second = breaker
            .observe_restore_outcome(&scope, &outcome, 11)
            .expect("second restore failure");
        assert!(second.opened);
        assert_eq!(
            breaker.decide(&scope, 12).expect("decide").permit,
            CircuitPermit::Suppressed
        );
    }

    #[test]
    fn s5d_unknown_process_corruption_forces_global_observe_only() {
        let mut breaker = breaker("global");
        let a = scope("runtime_pm", "fw-a");
        let b = scope("backlight", "fw-a");
        breaker
            .trip_global("unisolatable state corruption", 10)
            .expect("trip global");
        assert!(breaker.is_global_open());
        assert_eq!(
            breaker.decide(&a, 11).expect("a").permit,
            CircuitPermit::Suppressed
        );
        assert_eq!(
            breaker.decide(&b, 11).expect("b").permit,
            CircuitPermit::Suppressed
        );
        assert_eq!(
            breaker
                .clear_all(1000)
                .expect_err("non-root global clear must fail")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn s5d_production_gate_emits_scoped_diagnostic_and_firmware_identity() {
        let kernel = MemoryKernel::new();
        kernel.write_raw(Path::new("/sys/class/dmi/id/bios_version"), "TEST-FW-1\n");
        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test".to_string(),
        );
        let scope = CircuitScope::from_action(&action, &kernel);
        assert_eq!(scope.domain, "vm_sysctl");
        assert_eq!(scope.operation, "set_vm_sysctl");
        assert_eq!(scope.target_id, action.stable_target_id());
        assert!(scope.hardware_id.starts_with("hwid:"));
        assert!(scope.firmware_id.starts_with("firmware:"));
        let outcome = circuit_suppressed_outcome(&action, "S5D circuit open");
        assert_eq!(outcome.domain, "vm_sysctl");
        assert!(outcome
            .targets
            .iter()
            .any(|target| target.detail.as_deref() == Some("S5D circuit open")));
    }
}
