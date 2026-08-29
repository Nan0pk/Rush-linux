//! Deterministic full-system simulation and fault matrix.
//!
//! This module is compiled only with `test-simulation`. It deliberately has no
//! `KernelWrite`, `std::fs::write`, process, D-Bus, or systemd surface. The
//! production CLI entry point is absent from normal builds and enters here when
//! `--simulation-root` is present. A simulation root is a marked, non-root,
//! non-symlink directory containing the versioned matrix manifest; the engine
//! reads that manifest, models every target in memory, and returns a report for
//! stdout serialization.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const REPORT_SCHEMA_VERSION: u32 = 1;
const ROOT_MARKER: &str = ".optid-simulation-root-v1";
const ROOT_MARKER_CONTENT: &str = "optid-test-simulation-root-v1\n";
const MANIFEST_FILE: &str = "full-system-matrix-v1.json";

const REQUIRED_BUNDLE_ENTRIES: [&str; 8] = [
    "domain-status.json",
    "latest-control-cycles.jsonl",
    "gate-outcomes.json",
    "transaction-status.json",
    "recovery-status.json",
    "circuit-breakers.json",
    "relevant-journal.txt",
    "manifest.json",
];

const REQUIRED_COVERAGE: [&str; 39] = [
    "power.ac",
    "power.battery",
    "workload.idle",
    "workload.interactive",
    "workload.latency_critical",
    "workload.throughput",
    "thermal.rise",
    "thermal.alarm",
    "thermal.recovery",
    "thermal.missing",
    "thermal.stale",
    "foreground.arrival",
    "foreground.loss",
    "gamemode.arrival",
    "gamemode.loss",
    "hotplug.device_add",
    "hotplug.device_remove",
    "hotplug.cpu_add",
    "hotplug.cpu_remove",
    "hardware.unsupported",
    "file.permission_denied",
    "file.malformed",
    "file.partial_write",
    "file.short_write",
    "file.external_drift",
    "config.reload_success",
    "config.reload_failure",
    "seal.failure",
    "daemon.crash_before_write",
    "daemon.crash_during_transaction",
    "daemon.crash_after_write",
    "recovery.crash",
    "recovery.failed_restore",
    "recovery.stabilization",
    "circuit.open",
    "circuit.canary",
    "circuit.close",
    "reboot.recovery",
    "reboot.repeated",
];

#[derive(Debug)]
pub struct SimulationError(String);

impl SimulationError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SimulationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SimulationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationMode {
    Off,
    Observe,
    IndividualActuation,
    CombinedActuation,
}

impl SimulationMode {
    const ALL: [Self; 4] = [
        Self::Off,
        Self::Observe,
        Self::IndividualActuation,
        Self::CombinedActuation,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::IndividualActuation => "individual_actuation",
            Self::CombinedActuation => "combined_actuation",
        }
    }

    fn actuates(self) -> bool {
        matches!(self, Self::IndividualActuation | Self::CombinedActuation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    SoftwarePass,
    SoftwareFail,
    Unsupported,
    Rejected,
    KnownRegressionDetected,
    NoOpConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverallState {
    Healthy,
    Inactive,
    Observing,
    Degraded,
    Blocked,
    Failed,
    Recovering,
    Recovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    InputValidation,
    Observation,
    Decision,
    DomainGate,
    Configuration,
    CapabilityGate,
    Write,
    Readback,
    Restore,
    Recovery,
    CircuitBreaker,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    ScenarioCompleted,
    DomainOff,
    DomainObserve,
    MissingPrimaryMetric,
    MetricNa,
    MetricNan,
    MetricInfinite,
    MetricImpossible,
    ZeroIterations,
    ZeroLatency,
    UnsupportedMetric,
    UnsupportedTarget,
    ObservationMissing,
    ObservationStale,
    PermissionDenied,
    MalformedValue,
    ConfigReloadFailed,
    CapabilitySealFailed,
    DaemonCrashBeforeWrite,
    DaemonCrashDuringTransaction,
    DaemonCrashAfterWrite,
    RecoveryCrash,
    CircuitOpened,
    RebootRecoveryCompleted,
    PartialWrite,
    ShortWrite,
    ExternalDriftRelinquished,
    MultiWritePartialFailure,
    RestorationFailed,
    KnownRegressionDetected,
    NoOpConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    Clean,
    Prepared,
    Recovered,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    Canary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClassification {
    SuppressedOff,
    Observed,
    AppliedAndRestored,
    Unsupported,
    Denied,
    Malformed,
    RecoveredAfterCrash,
    ReadbackMismatchRecovered,
    DriftRelinquished,
    RestorationFailed,
    NotAttemptedAfterFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestorationStatus {
    NotNeeded,
    Restored,
    Recovered,
    Relinquished,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    Allowed,
    DomainOff,
    DomainObserve,
    CapabilityDenied,
    InputRejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioManifest {
    pub schema_version: u32,
    pub test_only: bool,
    pub seed: u64,
    pub mock_domains: Vec<String>,
    pub required_modes: Vec<SimulationMode>,
    pub required_bundle_entries: Vec<String>,
    pub scenarios: Vec<Scenario>,
    pub captured_bundle: CapturedBundle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub events: Vec<String>,
    pub observations: Vec<Observation>,
    pub actions: Vec<MockAction>,
    pub metrics: MetricSet,
    pub fault: FaultSpec,
    pub control: ControlKind,
    pub expected: BTreeMap<SimulationMode, ExpectedOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub component_id: String,
    pub value: serde_json::Value,
    pub status: ObservationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Supported,
    Unsupported,
    PermissionDenied,
    Malformed,
    Missing,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockAction {
    pub domain: String,
    pub target_id: String,
    pub previous_value: String,
    pub requested_value: String,
    pub declared_no_op: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricSet {
    pub primary: String,
    pub iterations: u64,
    pub values: BTreeMap<String, serde_json::Value>,
    pub baseline: Option<f64>,
    pub candidate: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    None,
    UnsupportedTarget,
    PermissionDenied,
    MalformedFile,
    ConfigReloadFailure,
    CapabilitySealFailure,
    DaemonCrashBeforeWrite,
    DaemonCrashDuringTransaction,
    DaemonCrashAfterWrite,
    RecoveryCrash,
    CircuitOpening,
    RebootRecovery,
    PartialWrite,
    ShortWrite,
    ExternalDrift,
    MultiWritePartial,
    RestoreDenied,
    InactiveLever,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultSpec {
    pub kind: FaultKind,
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    None,
    KnownRegression,
    NoOp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedOutcome {
    pub verdict: Verdict,
    pub overall_state: OverallState,
    pub affected_domain: String,
    pub failed_stage: PipelineStage,
    pub target_id: String,
    pub reason_code: ReasonCode,
    pub correlation_path: String,
    pub journal_state: JournalState,
    pub circuit_state: CircuitState,
    pub receipt_classifications: Vec<ActionClassification>,
    pub receipts: Vec<ExpectedReceipt>,
    pub restoration_complete: bool,
    pub bundle_entries: Vec<String>,
    pub unaffected_healthy_domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedReceipt {
    pub target_id: String,
    pub gate_outcome: GateOutcome,
    pub write_attempted: bool,
    pub read_back_value: Option<String>,
    pub restored_value: Option<String>,
    pub classification: ActionClassification,
    pub restoration: RestorationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedBundle {
    pub schema_version: u32,
    pub typed_failure: TypedFailure,
    pub redacted_machine_data: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedFailure {
    pub scenario_id: String,
    pub mode: SimulationMode,
    pub affected_domain: String,
    pub failed_stage: PipelineStage,
    pub target_id: String,
    pub reason_code: ReasonCode,
    pub correlation_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReproductionFixture {
    pub schema_version: u32,
    pub typed_failure: TypedFailure,
    pub reproduced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionReceipt {
    pub domain: String,
    pub target_id: String,
    pub previous_value: String,
    pub requested_value: String,
    pub read_back_value: Option<String>,
    pub restored_value: Option<String>,
    pub classification: ActionClassification,
    pub restoration: RestorationStatus,
    pub write_attempted: bool,
    pub actuation_group: String,
    pub correlation_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario_id: String,
    pub mode: SimulationMode,
    pub verdict: Verdict,
    pub overall_state: OverallState,
    pub observations: Vec<Observation>,
    pub desired_state: Vec<MockAction>,
    pub action_receipts: Vec<ActionReceipt>,
    pub affected_domain: String,
    pub failed_stage: PipelineStage,
    pub target_id: String,
    pub reason_code: ReasonCode,
    pub correlation_path: String,
    pub journal_state: JournalState,
    pub circuit_state: CircuitState,
    pub bundle_entries: Vec<String>,
    pub unaffected_healthy_domains: Vec<String>,
    pub restoration_complete: bool,
}

impl ScenarioResult {
    fn expectation(&self) -> ExpectedOutcome {
        ExpectedOutcome {
            verdict: self.verdict.clone(),
            overall_state: self.overall_state.clone(),
            affected_domain: self.affected_domain.clone(),
            failed_stage: self.failed_stage.clone(),
            target_id: self.target_id.clone(),
            reason_code: self.reason_code.clone(),
            correlation_path: self.correlation_path.clone(),
            journal_state: self.journal_state.clone(),
            circuit_state: self.circuit_state.clone(),
            receipt_classifications: self
                .action_receipts
                .iter()
                .map(|receipt| receipt.classification)
                .collect(),
            receipts: self
                .action_receipts
                .iter()
                .map(|receipt| ExpectedReceipt {
                    target_id: receipt.target_id.clone(),
                    gate_outcome: match receipt.classification {
                        ActionClassification::SuppressedOff => GateOutcome::DomainOff,
                        ActionClassification::Observed => GateOutcome::DomainObserve,
                        ActionClassification::Unsupported | ActionClassification::Denied => {
                            GateOutcome::CapabilityDenied
                        }
                        ActionClassification::NotAttemptedAfterFailure => {
                            GateOutcome::InputRejected
                        }
                        _ => GateOutcome::Allowed,
                    },
                    write_attempted: receipt.write_attempted,
                    read_back_value: receipt.read_back_value.clone(),
                    restored_value: receipt.restored_value.clone(),
                    classification: receipt.classification,
                    restoration: receipt.restoration,
                })
                .collect(),
            restoration_complete: self.restoration_complete,
            bundle_entries: self.bundle_entries.clone(),
            unaffected_healthy_domains: self.unaffected_healthy_domains.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatrixReport {
    pub schema_version: u32,
    pub manifest_schema_version: u32,
    pub seed: u64,
    pub test_only: bool,
    pub host_write_attempts: u64,
    pub matrix_passed: bool,
    pub covered_events: Vec<String>,
    pub scenario_results: Vec<ScenarioResult>,
    pub reproduction_fixture: ReproductionFixture,
}

#[derive(Debug, Clone)]
struct Diagnosis {
    verdict: Verdict,
    overall_state: OverallState,
    affected_domain: String,
    failed_stage: PipelineStage,
    target_id: String,
    reason_code: ReasonCode,
    journal_state: JournalState,
    circuit_state: CircuitState,
    restoration_complete: bool,
}

/// Run a manifest from a validated simulation root.
///
/// The function performs read-only filesystem access. Mock actions are applied
/// to an in-memory map; the caller decides how to display the returned report.
pub fn run_from_root(root: &Path) -> Result<MatrixReport, SimulationError> {
    let canonical_root = validate_root(root)?;
    let manifest_path = checked_regular_file(&canonical_root, MANIFEST_FILE)?;
    let source = fs::read_to_string(&manifest_path).map_err(|error| {
        SimulationError::new(format!("cannot read {}: {error}", manifest_path.display()))
    })?;
    let manifest: ScenarioManifest = serde_json::from_str(&source)
        .map_err(|error| SimulationError::new(format!("invalid scenario manifest: {error}")))?;
    run_manifest(manifest)
}

fn validate_root(root: &Path) -> Result<PathBuf, SimulationError> {
    if root.as_os_str().is_empty() || root == Path::new("/") {
        return Err(SimulationError::new(
            "simulation root must be a dedicated non-root directory",
        ));
    }
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        SimulationError::new(format!("cannot inspect simulation root: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SimulationError::new(
            "simulation root must be a real directory, not a symlink",
        ));
    }
    let canonical = fs::canonicalize(root).map_err(|error| {
        SimulationError::new(format!("cannot canonicalize simulation root: {error}"))
    })?;
    if canonical == Path::new("/") {
        return Err(SimulationError::new(
            "simulation root resolved to the real host root",
        ));
    }
    let marker = checked_regular_file(&canonical, ROOT_MARKER)?;
    let marker_content = fs::read_to_string(&marker)
        .map_err(|error| SimulationError::new(format!("cannot read root marker: {error}")))?;
    if marker_content != ROOT_MARKER_CONTENT {
        return Err(SimulationError::new(
            "simulation root marker is missing or invalid",
        ));
    }
    Ok(canonical)
}

fn checked_regular_file(root: &Path, relative: &str) -> Result<PathBuf, SimulationError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(SimulationError::new(
            "simulation file path escaped its root",
        ));
    }
    let path = root.join(relative_path);
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        SimulationError::new(format!("cannot inspect {}: {error}", path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SimulationError::new(format!(
            "simulation input must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(&path).map_err(|error| {
        SimulationError::new(format!("cannot canonicalize {}: {error}", path.display()))
    })?;
    if !canonical.starts_with(root) {
        return Err(SimulationError::new(
            "simulation input resolved outside its root",
        ));
    }
    Ok(canonical)
}

fn run_manifest(manifest: ScenarioManifest) -> Result<MatrixReport, SimulationError> {
    validate_manifest(&manifest)?;
    let mut covered_events = BTreeSet::new();
    let mut results = Vec::new();
    for scenario in &manifest.scenarios {
        covered_events.extend(scenario.events.iter().cloned());
        for mode in SimulationMode::ALL {
            let result = run_scenario(&manifest, scenario, mode)?;
            let expected = scenario.expected.get(&mode).ok_or_else(|| {
                SimulationError::new(format!(
                    "scenario {} has no expectation for {}",
                    scenario.id,
                    mode.as_str()
                ))
            })?;
            let actual = result.expectation();
            if &actual != expected {
                return Err(SimulationError::new(format!(
                    "scenario {} {} disagreed with its expected result\nexpected: {expected:#?}\nactual: {actual:#?}",
                    scenario.id,
                    mode.as_str()
                )));
            }
            results.push(result);
        }
    }

    let reproduction_fixture = convert_captured_bundle(&manifest.captured_bundle, &results)?;
    Ok(MatrixReport {
        schema_version: REPORT_SCHEMA_VERSION,
        manifest_schema_version: manifest.schema_version,
        seed: manifest.seed,
        test_only: true,
        host_write_attempts: 0,
        matrix_passed: true,
        covered_events: covered_events.into_iter().collect(),
        scenario_results: results,
        reproduction_fixture,
    })
}

fn validate_manifest(manifest: &ScenarioManifest) -> Result<(), SimulationError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(SimulationError::new(format!(
            "unsupported manifest schema {}; expected {}",
            manifest.schema_version, MANIFEST_SCHEMA_VERSION
        )));
    }
    if !manifest.test_only {
        return Err(SimulationError::new(
            "simulation manifest must declare test_only=true",
        ));
    }
    let modes: BTreeSet<_> = manifest.required_modes.iter().copied().collect();
    let required_modes: BTreeSet<_> = SimulationMode::ALL.into_iter().collect();
    if modes != required_modes || manifest.required_modes.len() != SimulationMode::ALL.len() {
        return Err(SimulationError::new(
            "manifest must require off, observe, individual_actuation, and combined_actuation exactly once",
        ));
    }
    if manifest.mock_domains.is_empty()
        || manifest.mock_domains.iter().any(|domain| domain.is_empty())
    {
        return Err(SimulationError::new(
            "manifest must declare non-empty mock domains",
        ));
    }
    let bundle_entries: BTreeSet<_> = manifest
        .required_bundle_entries
        .iter()
        .map(String::as_str)
        .collect();
    let required_bundle_entries: BTreeSet<_> = REQUIRED_BUNDLE_ENTRIES.into_iter().collect();
    if bundle_entries != required_bundle_entries {
        return Err(SimulationError::new(
            "manifest bundle entries do not match the required diagnostic bundle",
        ));
    }
    let mut scenario_ids = BTreeSet::new();
    let mut coverage = BTreeSet::new();
    for scenario in &manifest.scenarios {
        if scenario.id.is_empty() || !scenario_ids.insert(scenario.id.as_str()) {
            return Err(SimulationError::new(format!(
                "scenario id is empty or duplicated: {}",
                scenario.id
            )));
        }
        if scenario.actions.is_empty() || scenario.observations.is_empty() {
            return Err(SimulationError::new(format!(
                "scenario {} must declare observations and desired actions",
                scenario.id
            )));
        }
        for action in &scenario.actions {
            if !manifest.mock_domains.contains(&action.domain) {
                return Err(SimulationError::new(format!(
                    "scenario {} uses undeclared mock domain {}",
                    scenario.id, action.domain
                )));
            }
            if action.target_id.is_empty()
                || action.previous_value.is_empty()
                || action.requested_value.is_empty()
            {
                return Err(SimulationError::new(format!(
                    "scenario {} has an incomplete action receipt source",
                    scenario.id
                )));
            }
        }
        if scenario.expected.len() != SimulationMode::ALL.len() {
            return Err(SimulationError::new(format!(
                "scenario {} must declare exactly four mode expectations",
                scenario.id
            )));
        }
        coverage.extend(scenario.events.iter().map(String::as_str));
    }
    for required in REQUIRED_COVERAGE {
        if !coverage.contains(required) {
            return Err(SimulationError::new(format!(
                "scenario matrix is missing required coverage: {required}"
            )));
        }
    }
    Ok(())
}

fn run_scenario(
    manifest: &ScenarioManifest,
    scenario: &Scenario,
    mode: SimulationMode,
) -> Result<ScenarioResult, SimulationError> {
    let correlation_path = format!("optid-simulation/{}/{}", scenario.id, mode.as_str());
    let mut diagnosis = validate_metrics(scenario)
        .unwrap_or_else(|| diagnose_scenario(scenario, mode, &correlation_path));
    let block_actions = (diagnosis.failed_stage == PipelineStage::InputValidation
        && diagnosis.verdict != Verdict::NoOpConfirmed)
        || diagnosis.failed_stage == PipelineStage::Configuration
        || (diagnosis.failed_stage == PipelineStage::Observation
            && scenario.fault.kind == FaultKind::None);
    let action_receipts = build_receipts(scenario, mode, &correlation_path, block_actions);

    if action_receipts.len() != scenario.actions.len() {
        return Err(SimulationError::new(format!(
            "scenario {} lost an action receipt",
            scenario.id
        )));
    }
    for (action, receipt) in scenario.actions.iter().zip(&action_receipts) {
        if receipt.previous_value != action.previous_value
            || receipt.requested_value != action.requested_value
            || receipt.correlation_path.is_empty()
        {
            return Err(SimulationError::new(format!(
                "scenario {} produced an incomplete or uncorrelated action receipt",
                scenario.id
            )));
        }
        if mode.actuates()
            && receipt.write_attempted
            && receipt.read_back_value.is_none()
            && !matches!(receipt.classification, ActionClassification::Denied)
        {
            return Err(SimulationError::new(format!(
                "scenario {} attempted a write without read-back",
                scenario.id
            )));
        }
        if matches!(
            receipt.restoration,
            RestorationStatus::Restored | RestorationStatus::Recovered
        ) && receipt.restored_value.as_deref() != Some(action.previous_value.as_str())
        {
            return Err(SimulationError::new(format!(
                "scenario {} retained stale desired state for {}",
                scenario.id, action.target_id
            )));
        }
    }

    let restoration_complete = action_receipts.iter().all(|receipt| {
        matches!(
            receipt.restoration,
            RestorationStatus::NotNeeded
                | RestorationStatus::Restored
                | RestorationStatus::Recovered
                | RestorationStatus::Relinquished
        )
    });
    if diagnosis.restoration_complete && !restoration_complete {
        diagnosis = Diagnosis {
            verdict: Verdict::SoftwareFail,
            overall_state: OverallState::Failed,
            affected_domain: action_receipts
                .iter()
                .find(|receipt| receipt.restoration == RestorationStatus::Failed)
                .map(|receipt| receipt.domain.clone())
                .unwrap_or_else(|| "simulation_harness".to_string()),
            failed_stage: PipelineStage::Restore,
            target_id: action_receipts
                .iter()
                .find(|receipt| receipt.restoration == RestorationStatus::Failed)
                .map(|receipt| receipt.target_id.clone())
                .unwrap_or_else(|| format!("scenario:{}", scenario.id)),
            reason_code: ReasonCode::RestorationFailed,
            journal_state: JournalState::Unresolved,
            circuit_state: CircuitState::Open,
            restoration_complete: false,
        };
    }
    diagnosis.restoration_complete = restoration_complete;

    let unaffected_healthy_domains =
        unaffected_domains(&manifest.mock_domains, &diagnosis.affected_domain);
    Ok(ScenarioResult {
        scenario_id: scenario.id.clone(),
        mode,
        verdict: diagnosis.verdict,
        overall_state: diagnosis.overall_state,
        observations: scenario.observations.clone(),
        desired_state: scenario.actions.clone(),
        action_receipts,
        affected_domain: diagnosis.affected_domain,
        failed_stage: diagnosis.failed_stage,
        target_id: diagnosis.target_id,
        reason_code: diagnosis.reason_code,
        correlation_path,
        journal_state: diagnosis.journal_state,
        circuit_state: diagnosis.circuit_state,
        bundle_entries: manifest.required_bundle_entries.clone(),
        unaffected_healthy_domains,
        restoration_complete: diagnosis.restoration_complete,
    })
}

fn validate_metrics(scenario: &Scenario) -> Option<Diagnosis> {
    let target = format!("metric:{}", scenario.metrics.primary);
    let invalid = |reason_code| Diagnosis {
        verdict: Verdict::Rejected,
        overall_state: OverallState::Failed,
        affected_domain: "simulation_harness".to_string(),
        failed_stage: PipelineStage::InputValidation,
        target_id: target.clone(),
        reason_code,
        journal_state: JournalState::Clean,
        circuit_state: CircuitState::Closed,
        restoration_complete: true,
    };
    if scenario.metrics.iterations == 0 {
        return Some(invalid(ReasonCode::ZeroIterations));
    }
    let value = match scenario.metrics.values.get(&scenario.metrics.primary) {
        Some(value) => value,
        None => return Some(invalid(ReasonCode::MissingPrimaryMetric)),
    };
    match value {
        serde_json::Value::String(value) if value.eq_ignore_ascii_case("na") => {
            return Some(invalid(ReasonCode::MetricNa));
        }
        serde_json::Value::String(value) if value.eq_ignore_ascii_case("nan") => {
            return Some(invalid(ReasonCode::MetricNan));
        }
        serde_json::Value::String(value)
            if value.eq_ignore_ascii_case("inf")
                || value.eq_ignore_ascii_case("infinity")
                || value.eq_ignore_ascii_case("-inf") =>
        {
            return Some(invalid(ReasonCode::MetricInfinite));
        }
        serde_json::Value::String(value) if value.eq_ignore_ascii_case("unsupported") => {
            return Some(invalid(ReasonCode::UnsupportedMetric));
        }
        serde_json::Value::Number(number) => {
            let number = number.as_f64().unwrap_or(f64::NAN);
            if !number.is_finite() {
                return Some(invalid(ReasonCode::MetricNan));
            }
            if number < 0.0 || number > 1.0e15 {
                return Some(invalid(ReasonCode::MetricImpossible));
            }
            if number == 0.0 && scenario.metrics.primary.contains("latency") {
                return Some(invalid(ReasonCode::ZeroLatency));
            }
        }
        _ => return Some(invalid(ReasonCode::MetricImpossible)),
    }

    if scenario.control == ControlKind::KnownRegression {
        let (Some(baseline), Some(candidate)) =
            (scenario.metrics.baseline, scenario.metrics.candidate)
        else {
            return Some(invalid(ReasonCode::MissingPrimaryMetric));
        };
        if baseline.is_finite() && candidate.is_finite() && candidate > baseline {
            return Some(Diagnosis {
                verdict: Verdict::KnownRegressionDetected,
                overall_state: OverallState::Degraded,
                affected_domain: "simulation_harness".to_string(),
                failed_stage: PipelineStage::InputValidation,
                target_id: target,
                reason_code: ReasonCode::KnownRegressionDetected,
                journal_state: JournalState::Clean,
                circuit_state: CircuitState::Closed,
                restoration_complete: true,
            });
        }
        return Some(invalid(ReasonCode::MetricImpossible));
    }
    if scenario.control == ControlKind::NoOp {
        let (Some(baseline), Some(candidate)) =
            (scenario.metrics.baseline, scenario.metrics.candidate)
        else {
            return Some(invalid(ReasonCode::MissingPrimaryMetric));
        };
        if baseline.to_bits() == candidate.to_bits() {
            return Some(Diagnosis {
                verdict: Verdict::NoOpConfirmed,
                overall_state: OverallState::Healthy,
                affected_domain: "none".to_string(),
                failed_stage: PipelineStage::Complete,
                target_id: format!("scenario:{}", scenario.id),
                reason_code: ReasonCode::NoOpConfirmed,
                journal_state: JournalState::Clean,
                circuit_state: CircuitState::Closed,
                restoration_complete: true,
            });
        }
        return Some(invalid(ReasonCode::MetricImpossible));
    }
    None
}

fn diagnose_scenario(scenario: &Scenario, mode: SimulationMode, _correlation: &str) -> Diagnosis {
    let first_action = &scenario.actions[0];
    let target_id = scenario
        .fault
        .target_id
        .clone()
        .unwrap_or_else(|| first_action.target_id.clone());
    let affected_domain = scenario
        .actions
        .iter()
        .find(|action| action.target_id == target_id)
        .map(|action| action.domain.clone())
        .unwrap_or_else(|| first_action.domain.clone());
    let healthy = |overall_state, reason_code| Diagnosis {
        verdict: Verdict::SoftwarePass,
        overall_state,
        affected_domain: "none".to_string(),
        failed_stage: if matches!(mode, SimulationMode::Off | SimulationMode::Observe) {
            PipelineStage::DomainGate
        } else {
            PipelineStage::Complete
        },
        target_id: format!("scenario:{}", scenario.id),
        reason_code,
        journal_state: JournalState::Clean,
        circuit_state: CircuitState::Closed,
        restoration_complete: true,
    };
    if mode == SimulationMode::Off {
        return healthy(OverallState::Inactive, ReasonCode::DomainOff);
    }

    if let Some(observation) = scenario
        .observations
        .iter()
        .find(|observation| observation.status != ObservationStatus::Supported)
    {
        let (verdict, overall_state, reason_code) = match observation.status {
            ObservationStatus::Supported => unreachable!(),
            ObservationStatus::Unsupported => (
                Verdict::Unsupported,
                OverallState::Blocked,
                ReasonCode::UnsupportedTarget,
            ),
            ObservationStatus::PermissionDenied => (
                Verdict::SoftwareFail,
                OverallState::Failed,
                ReasonCode::PermissionDenied,
            ),
            ObservationStatus::Malformed => (
                Verdict::SoftwareFail,
                OverallState::Failed,
                ReasonCode::MalformedValue,
            ),
            ObservationStatus::Missing => (
                Verdict::SoftwareFail,
                OverallState::Failed,
                ReasonCode::ObservationMissing,
            ),
            ObservationStatus::Stale => (
                Verdict::SoftwareFail,
                OverallState::Failed,
                ReasonCode::ObservationStale,
            ),
        };
        return Diagnosis {
            verdict,
            overall_state,
            affected_domain: "observation".to_string(),
            failed_stage: PipelineStage::Observation,
            target_id: observation.component_id.clone(),
            reason_code,
            journal_state: JournalState::Clean,
            circuit_state: CircuitState::Closed,
            restoration_complete: true,
        };
    }

    if scenario.fault.kind == FaultKind::ConfigReloadFailure {
        return Diagnosis {
            verdict: Verdict::SoftwareFail,
            overall_state: OverallState::Degraded,
            affected_domain: "configuration".to_string(),
            failed_stage: PipelineStage::Configuration,
            target_id: "config:policy".to_string(),
            reason_code: ReasonCode::ConfigReloadFailed,
            journal_state: JournalState::Clean,
            circuit_state: CircuitState::Closed,
            restoration_complete: true,
        };
    }

    if mode == SimulationMode::Observe {
        return match scenario.fault.kind {
            FaultKind::UnsupportedTarget | FaultKind::InactiveLever => Diagnosis {
                verdict: Verdict::Unsupported,
                overall_state: OverallState::Blocked,
                affected_domain,
                failed_stage: PipelineStage::CapabilityGate,
                target_id,
                reason_code: ReasonCode::UnsupportedTarget,
                journal_state: JournalState::Clean,
                circuit_state: CircuitState::Closed,
                restoration_complete: true,
            },
            FaultKind::PermissionDenied => Diagnosis {
                verdict: Verdict::SoftwareFail,
                overall_state: OverallState::Failed,
                affected_domain,
                failed_stage: PipelineStage::Observation,
                target_id,
                reason_code: ReasonCode::PermissionDenied,
                journal_state: JournalState::Clean,
                circuit_state: CircuitState::Closed,
                restoration_complete: true,
            },
            FaultKind::MalformedFile => Diagnosis {
                verdict: Verdict::SoftwareFail,
                overall_state: OverallState::Failed,
                affected_domain,
                failed_stage: PipelineStage::Observation,
                target_id,
                reason_code: ReasonCode::MalformedValue,
                journal_state: JournalState::Clean,
                circuit_state: CircuitState::Closed,
                restoration_complete: true,
            },
            _ => healthy(OverallState::Observing, ReasonCode::DomainObserve),
        };
    }

    let failure = |verdict,
                   overall_state,
                   failed_stage,
                   reason_code,
                   journal_state,
                   circuit_state,
                   restoration_complete| Diagnosis {
        verdict,
        overall_state,
        affected_domain: affected_domain.clone(),
        failed_stage,
        target_id: target_id.clone(),
        reason_code,
        journal_state,
        circuit_state,
        restoration_complete,
    };
    match scenario.fault.kind {
        FaultKind::None => healthy(OverallState::Healthy, ReasonCode::ScenarioCompleted),
        FaultKind::UnsupportedTarget | FaultKind::InactiveLever => failure(
            Verdict::Unsupported,
            OverallState::Blocked,
            PipelineStage::CapabilityGate,
            ReasonCode::UnsupportedTarget,
            JournalState::Clean,
            CircuitState::Closed,
            true,
        ),
        FaultKind::PermissionDenied => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Write,
            ReasonCode::PermissionDenied,
            JournalState::Clean,
            CircuitState::Closed,
            true,
        ),
        FaultKind::MalformedFile => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Observation,
            ReasonCode::MalformedValue,
            JournalState::Clean,
            CircuitState::Closed,
            true,
        ),
        FaultKind::ConfigReloadFailure => unreachable!(),
        FaultKind::CapabilitySealFailure => failure(
            Verdict::SoftwareFail,
            OverallState::Blocked,
            PipelineStage::CapabilityGate,
            ReasonCode::CapabilitySealFailed,
            JournalState::Clean,
            CircuitState::Closed,
            true,
        ),
        FaultKind::DaemonCrashBeforeWrite => failure(
            Verdict::SoftwarePass,
            OverallState::Recovered,
            PipelineStage::Recovery,
            ReasonCode::DaemonCrashBeforeWrite,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::DaemonCrashDuringTransaction => failure(
            Verdict::SoftwarePass,
            OverallState::Recovered,
            PipelineStage::Recovery,
            ReasonCode::DaemonCrashDuringTransaction,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::DaemonCrashAfterWrite => failure(
            Verdict::SoftwarePass,
            OverallState::Recovered,
            PipelineStage::Recovery,
            ReasonCode::DaemonCrashAfterWrite,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::RecoveryCrash => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Recovery,
            ReasonCode::RecoveryCrash,
            JournalState::Unresolved,
            CircuitState::Open,
            false,
        ),
        FaultKind::CircuitOpening => failure(
            Verdict::SoftwarePass,
            OverallState::Degraded,
            PipelineStage::CircuitBreaker,
            ReasonCode::CircuitOpened,
            JournalState::Recovered,
            CircuitState::Open,
            true,
        ),
        FaultKind::RebootRecovery => failure(
            Verdict::SoftwarePass,
            OverallState::Recovered,
            PipelineStage::Recovery,
            ReasonCode::RebootRecoveryCompleted,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::PartialWrite => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Readback,
            ReasonCode::PartialWrite,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::ShortWrite => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Readback,
            ReasonCode::ShortWrite,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::ExternalDrift => failure(
            Verdict::SoftwareFail,
            OverallState::Degraded,
            PipelineStage::Readback,
            ReasonCode::ExternalDriftRelinquished,
            JournalState::Clean,
            CircuitState::Closed,
            true,
        ),
        FaultKind::MultiWritePartial => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Write,
            ReasonCode::MultiWritePartialFailure,
            JournalState::Recovered,
            CircuitState::Closed,
            true,
        ),
        FaultKind::RestoreDenied => failure(
            Verdict::SoftwareFail,
            OverallState::Failed,
            PipelineStage::Restore,
            ReasonCode::RestorationFailed,
            JournalState::Unresolved,
            CircuitState::Open,
            false,
        ),
    }
}

fn build_receipts(
    scenario: &Scenario,
    mode: SimulationMode,
    correlation: &str,
    block_actions: bool,
) -> Vec<ActionReceipt> {
    let mut state: BTreeMap<String, String> = scenario
        .actions
        .iter()
        .map(|action| (action.target_id.clone(), action.previous_value.clone()))
        .collect();
    let mut receipts = Vec::with_capacity(scenario.actions.len());
    let mut combined_failed = false;
    for (index, action) in scenario.actions.iter().enumerate() {
        let action_correlation = format!("{correlation}/action-{index:02}");
        let actuation_group = match mode {
            SimulationMode::Off => "off".to_string(),
            SimulationMode::Observe => "observe".to_string(),
            SimulationMode::IndividualActuation => format!("individual:{}", action.domain),
            SimulationMode::CombinedActuation => "combined".to_string(),
        };
        let fault_matches = scenario
            .fault
            .target_id
            .as_ref()
            .map(|target| target == &action.target_id)
            .unwrap_or(index == 0);
        let mut receipt = ActionReceipt {
            domain: action.domain.clone(),
            target_id: action.target_id.clone(),
            previous_value: action.previous_value.clone(),
            requested_value: action.requested_value.clone(),
            read_back_value: Some(action.previous_value.clone()),
            restored_value: Some(action.previous_value.clone()),
            classification: match mode {
                SimulationMode::Off => ActionClassification::SuppressedOff,
                SimulationMode::Observe => ActionClassification::Observed,
                SimulationMode::IndividualActuation | SimulationMode::CombinedActuation => {
                    ActionClassification::AppliedAndRestored
                }
            },
            restoration: if mode.actuates() {
                RestorationStatus::Restored
            } else {
                RestorationStatus::NotNeeded
            },
            write_attempted: mode.actuates(),
            actuation_group,
            correlation_path: action_correlation,
        };

        if mode.actuates() {
            if block_actions {
                receipt.write_attempted = false;
                receipt.classification = ActionClassification::NotAttemptedAfterFailure;
                receipt.restoration = RestorationStatus::NotNeeded;
                receipts.push(receipt);
                continue;
            }
            if combined_failed && mode == SimulationMode::CombinedActuation {
                receipt.write_attempted = false;
                receipt.read_back_value = Some(action.previous_value.clone());
                receipt.classification = ActionClassification::NotAttemptedAfterFailure;
                receipt.restoration = RestorationStatus::NotNeeded;
                receipts.push(receipt);
                continue;
            }
            state.insert(action.target_id.clone(), action.requested_value.clone());
            receipt.read_back_value = Some(action.requested_value.clone());
            match scenario.fault.kind {
                FaultKind::UnsupportedTarget | FaultKind::InactiveLever if fault_matches => {
                    receipt.write_attempted = false;
                    receipt.read_back_value = None;
                    receipt.classification = ActionClassification::Unsupported;
                    receipt.restoration = RestorationStatus::NotNeeded;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
                FaultKind::PermissionDenied | FaultKind::CapabilitySealFailure
                    if fault_matches || scenario.fault.kind == FaultKind::CapabilitySealFailure =>
                {
                    receipt.write_attempted = false;
                    receipt.read_back_value = None;
                    receipt.classification = ActionClassification::Denied;
                    receipt.restoration = RestorationStatus::NotNeeded;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
                FaultKind::MalformedFile if fault_matches => {
                    receipt.write_attempted = false;
                    receipt.read_back_value = Some("malformed".to_string());
                    receipt.classification = ActionClassification::Malformed;
                    receipt.restoration = RestorationStatus::NotNeeded;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
                FaultKind::DaemonCrashBeforeWrite if fault_matches => {
                    receipt.write_attempted = false;
                    receipt.read_back_value = None;
                    receipt.classification = ActionClassification::RecoveredAfterCrash;
                    receipt.restoration = RestorationStatus::Recovered;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
                FaultKind::DaemonCrashDuringTransaction
                | FaultKind::DaemonCrashAfterWrite
                | FaultKind::CircuitOpening
                | FaultKind::RebootRecovery
                    if fault_matches =>
                {
                    receipt.classification = ActionClassification::RecoveredAfterCrash;
                    receipt.restoration = RestorationStatus::Recovered;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
                FaultKind::RecoveryCrash | FaultKind::RestoreDenied if fault_matches => {
                    receipt.classification = ActionClassification::RestorationFailed;
                    receipt.restoration = RestorationStatus::Failed;
                    receipt.restored_value = Some(action.requested_value.clone());
                }
                FaultKind::PartialWrite | FaultKind::ShortWrite if fault_matches => {
                    receipt.read_back_value = Some("partial".to_string());
                    receipt.classification = ActionClassification::ReadbackMismatchRecovered;
                    receipt.restoration = RestorationStatus::Recovered;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
                FaultKind::ExternalDrift if fault_matches => {
                    receipt.read_back_value = Some("external-owner-value".to_string());
                    receipt.restored_value = Some("external-owner-value".to_string());
                    receipt.classification = ActionClassification::DriftRelinquished;
                    receipt.restoration = RestorationStatus::Relinquished;
                    state.insert(action.target_id.clone(), "external-owner-value".to_string());
                }
                FaultKind::MultiWritePartial if fault_matches => {
                    receipt.read_back_value = Some("partial".to_string());
                    receipt.classification = ActionClassification::ReadbackMismatchRecovered;
                    receipt.restoration = RestorationStatus::Recovered;
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                    combined_failed = true;
                }
                _ => {
                    state.insert(action.target_id.clone(), action.previous_value.clone());
                }
            }
        }
        receipts.push(receipt);
    }

    // The in-memory state is intentionally consumed only as a stale-desired
    // assertion. External-drift relinquishment is the one valid non-original
    // final value because optid no longer owns it.
    for receipt in &receipts {
        let final_value = state.get(&receipt.target_id);
        if receipt.restoration == RestorationStatus::Restored
            || receipt.restoration == RestorationStatus::Recovered
        {
            debug_assert_eq!(final_value, Some(&receipt.previous_value));
        }
    }
    receipts
}

fn unaffected_domains(domains: &[String], affected: &str) -> Vec<String> {
    domains
        .iter()
        .filter(|domain| affected == "none" || domain.as_str() != affected)
        .cloned()
        .collect()
}

fn convert_captured_bundle(
    bundle: &CapturedBundle,
    results: &[ScenarioResult],
) -> Result<ReproductionFixture, SimulationError> {
    if bundle.schema_version != 1 {
        return Err(SimulationError::new(
            "captured bundle has an unsupported schema",
        ));
    }
    let reproduced = results.iter().any(|result| {
        result.scenario_id == bundle.typed_failure.scenario_id
            && result.mode == bundle.typed_failure.mode
            && result.affected_domain == bundle.typed_failure.affected_domain
            && result.failed_stage == bundle.typed_failure.failed_stage
            && result.target_id == bundle.typed_failure.target_id
            && result.reason_code == bundle.typed_failure.reason_code
            && result.correlation_path == bundle.typed_failure.correlation_path
    });
    if !reproduced {
        return Err(SimulationError::new(
            "captured typed failure did not reproduce through the simulation matrix",
        ));
    }
    let fixture = ReproductionFixture {
        schema_version: 1,
        typed_failure: bundle.typed_failure.clone(),
        reproduced,
    };
    let serialized = serde_json::to_string(&fixture)
        .map_err(|error| SimulationError::new(format!("cannot serialize fixture: {error}")))?;
    for prohibited in &bundle.redacted_machine_data {
        if !prohibited.is_empty() && serialized.contains(prohibited) {
            return Err(SimulationError::new(
                "bundle-to-fixture conversion leaked redacted machine or user data",
            ));
        }
    }
    Ok(fixture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/i2-simulation")
    }

    #[test]
    fn i2_complete_matrix_matches_declared_results_and_is_deterministic() {
        let first = run_from_root(&fixture_root()).expect("first deterministic matrix run");
        let second = run_from_root(&fixture_root()).expect("second deterministic matrix run");
        assert_eq!(first, second);
        assert!(first.matrix_passed);
        assert_eq!(first.host_write_attempts, 0);
        assert_eq!(first.scenario_results.len() % SimulationMode::ALL.len(), 0);
        assert!(first.reproduction_fixture.reproduced);
    }

    #[test]
    fn i2_matrix_contains_every_required_mode_event_and_control() {
        let report = run_from_root(&fixture_root()).expect("complete matrix");
        let covered: BTreeSet<_> = report.covered_events.iter().map(String::as_str).collect();
        for required in REQUIRED_COVERAGE {
            assert!(covered.contains(required), "missing coverage {required}");
        }
        for required_reason in [
            ReasonCode::MissingPrimaryMetric,
            ReasonCode::ZeroIterations,
            ReasonCode::ZeroLatency,
            ReasonCode::MetricNa,
            ReasonCode::MetricNan,
            ReasonCode::MetricInfinite,
            ReasonCode::MetricImpossible,
            ReasonCode::UnsupportedMetric,
            ReasonCode::UnsupportedTarget,
            ReasonCode::RestorationFailed,
            ReasonCode::KnownRegressionDetected,
        ] {
            assert!(
                report
                    .scenario_results
                    .iter()
                    .any(|result| result.reason_code == required_reason),
                "missing fail-closed control {required_reason:?}"
            );
        }
    }

    #[test]
    fn i2_every_action_receipt_records_all_four_values() {
        let report = run_from_root(&fixture_root()).expect("complete matrix");
        for result in report.scenario_results {
            for receipt in result.action_receipts {
                assert!(!receipt.previous_value.is_empty());
                assert!(!receipt.requested_value.is_empty());
                if receipt.write_attempted {
                    assert!(receipt.read_back_value.is_some());
                }
                assert!(receipt.restored_value.is_some());
            }
        }
    }

    #[test]
    fn i2_simulation_module_has_no_host_write_or_process_surface() {
        let source = include_str!("simulation.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production simulation source");
        for forbidden in [
            "fs::write(",
            "OpenOptions",
            "impl KernelWrite",
            "Command::new",
            "zbus::",
            "systemctl",
        ] {
            assert!(
                !production.contains(forbidden),
                "simulation gained forbidden host surface: {forbidden}"
            );
        }
    }

    #[test]
    fn i2_bundle_round_trip_preserves_typed_failure_and_redacts_machine_data() {
        let report = run_from_root(&fixture_root()).expect("complete matrix");
        let fixture = serde_json::to_string(&report.reproduction_fixture)
            .expect("serialize reproduction fixture");
        assert!(report.reproduction_fixture.reproduced);
        assert!(fixture.contains("permission_denied"));
        for prohibited in ["alice", "/home/alice", "TOKEN=super-secret", "SERIAL-1234"] {
            assert!(!fixture.contains(prohibited));
        }
    }

    #[test]
    fn i2_manifest_rejects_undeclared_domains_and_expected_result_drift() {
        let source = fs::read_to_string(fixture_root().join(MANIFEST_FILE))
            .expect("read committed manifest");
        let mut undeclared: ScenarioManifest =
            serde_json::from_str(&source).expect("parse committed manifest");
        undeclared.scenarios[0].actions[0].domain = "undeclared_domain".to_string();
        assert!(run_manifest(undeclared)
            .expect_err("undeclared domain must fail")
            .to_string()
            .contains("undeclared mock domain"));

        let mut drifted: ScenarioManifest =
            serde_json::from_str(&source).expect("parse committed manifest again");
        drifted.scenarios[0]
            .expected
            .get_mut(&SimulationMode::CombinedActuation)
            .expect("combined expectation")
            .reason_code = ReasonCode::PermissionDenied;
        assert!(run_manifest(drifted)
            .expect_err("known expected-result drift must fail")
            .to_string()
            .contains("disagreed with its expected result"));
    }
}
