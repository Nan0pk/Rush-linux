//! The `Actuator` — applies a `Decision`'s `Action` set behind the §3
//! actuation rule. Every write goes through `io_util::guarded_write` so the
//! allowlist is enforced at the single funnel. Every action that mutates a
//! sysfs/procfs value journals the original value into the state directory
//! so `revert_sysctls` / `revert_pm_qos` can restore it on shutdown.
//!
//! The PM QoS sink is abstracted behind `PmqosSink` so tests can inject a
//! fake sink instead of opening `/dev/cpu_dma_latency` (which requires
//! CAP_SYS_ADMIN on real kernels).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::action::Action;
use crate::actuators::{display, runtime_pm, storage};
use crate::allowlist::{
    hwid_from_ancestors, hwid_from_attr_path, hwid_from_device_dir, Allowlist, Verdict,
};
use crate::capability::Capability;
use crate::contracts::{
    contract_gate_device_resume_constraint, contract_gate_runtime_pm, ContractFloors,
    ContractGateResult, ExitLatencyEvidence,
};
use crate::envelope::{
    readback_from_result, ActionOutcome, ErrorKindCode, GateDisposition, GateEvaluation,
    GateReasonCode, GateStage, OutcomeReasonCode, OwnershipState, PipelineStage, ReadbackOutcome,
    ResponsibleSubsystem, RestoreOutcome, RestoreState, SupportState, TargetOutcome, WriteOutcome,
};
use crate::io_util::{
    append_log_with, atomic_write_state_file_with, clear_journal_with, get_path_hash,
    mark_applied_with,
};
use crate::kernel_io::{KernelIo, KernelWrite, RealKernel};
use crate::load_state::BootState;
use crate::sensors::discover_cpu_epp_paths_with;

pub(crate) trait PmqosSink {
    fn read_cpu_latency(&self) -> io::Result<String>;
    fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()>;
    fn read_device_latency(&self, device_path: &Path) -> io::Result<String>;
    fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()>;
}

pub(crate) struct RealPmqosSink {
    cpu_fd: Option<fs::File>,
}

impl RealPmqosSink {
    pub(crate) fn new() -> Self {
        Self { cpu_fd: None }
    }
}

impl PmqosSink for RealPmqosSink {
    fn read_cpu_latency(&self) -> io::Result<String> {
        let text = fs::read_to_string("/dev/cpu_dma_latency")?;
        Ok(text)
    }

    fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
        use std::io::Write;
        match value {
            Some(val) => {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/cpu_dma_latency")?;
                file.write_all(&val.to_ne_bytes())?;
                file.flush()?;
                self.cpu_fd = Some(file);
            }
            None => {
                self.cpu_fd = None;
            }
        }
        Ok(())
    }

    fn read_device_latency(&self, device_path: &Path) -> io::Result<String> {
        fs::read_to_string(device_path)
    }

    fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()> {
        crate::kernel_io::RealKernel::new().write(device_path, value)
    }
}

pub(crate) struct Actuator {
    pub(crate) state_dir: PathBuf,
    pub(crate) log_path: PathBuf,
    pub(crate) audit_path: PathBuf,
    pub(crate) pmqos_sink: Box<dyn PmqosSink>,
    pub(crate) last_cpu_latency: Option<Option<i32>>,
    pub(crate) last_device_latencies: HashMap<PathBuf, Option<i32>>,
    /// WP-N5: last-applied autosuspend delay per device dir, to skip redundant
    /// re-writes (and journal churn) within a session.
    pub(crate) last_runtime_pm: HashMap<PathBuf, i32>,
    /// WP-N6: last-applied PCIe ASPM enable state per device dir.
    pub(crate) last_pcie_aspm: HashMap<PathBuf, bool>,
    /// WP-N6: last-applied SATA ALPM policy per scsi_host dir.
    pub(crate) last_sata_alpm: HashMap<PathBuf, String>,
    /// WP-N7: last-applied raw backlight value per backlight device dir.
    pub(crate) last_backlight: HashMap<PathBuf, u64>,
    /// WP-N4 hardware allowlist gate. `None` ⇒ gate disabled (the v0.x default):
    /// the actuator behaves exactly as before. `Some(_)` ⇒ depth-enabler writes
    /// are default-denied unless the device HWID is allowlisted, and every
    /// denial is appended to `audit_path` with its reason.
    pub(crate) allowlist: Option<Allowlist>,
    /// optid-safety: the boot-time decision surface. `None` until
    /// `set_boot_state` is called from `main`. When `None`, the actuator
    /// behaves as before (dynamic writes gated only by `--apply` and the
    /// allowlist). When `Some(_)`, dynamic writes are additionally gated by
    /// `boot_state.apply_armed`. The curated baseline is gated by
    /// `boot_state.baseline_armed` and applied via `apply_baseline`.
    pub(crate) boot_state: Option<BootState>,
    /// SPEC §3 contract gate. The `ContractFloors` resolved from the
    /// committed workload class for this tick. `None` ⇒ no contract has
    /// been installed (legacy callers and unit tests that construct an
    /// `Actuator` directly), in which case the gate is open and the
    /// actuator behaves exactly as before. `main` calls
    /// `set_active_floors` every tick before applying the decision.
    pub(crate) active_floors: Option<ContractFloors>,
    /// F2: injectable kernel I/O. Defaults to `RealKernel` for production
    /// and existing tests. New fault-injection tests construct the actuator
    /// via `new_with_kernel` and pass a `FaultKernel` to simulate missing
    /// paths, permission-denied, short writes, and disappearing devices.
    pub(crate) kernel: Box<dyn KernelIo>,
    /// Correlation ID for the current control-loop iteration.
    pub(crate) correlation_id: String,
    /// Test-only hook: when `Some(n)`, the `n`-th `guarded_write` call within
    /// a single `Action::RuntimePm` apply (1 = delay write, 2 = control write,
    /// 3 = rollback delay write) returns a synthetic `Err`. This field is
    /// `#[cfg(test)]` — it does NOT exist in production builds, so there is
    /// zero test-hook state in the production binary.
    #[cfg(test)]
    pub(crate) fail_nth_runtime_pm_write: Option<usize>,
    /// Test-only hook: when `true`, `contract_permits` returns `Ok(true)`
    /// without evaluating the contract gate. This lets tests that
    /// exercise the actuator's apply/journal/rollback paths (e.g. the
    /// phase6 transactional-apply tests, the N4/N5 allowlist tests) do
    /// so without being blocked by the post-#338 fail-closed contract
    /// gate (which denies depth-enablers when `active_floors` is `None`
    /// and denies RuntimePm when C1 evidence is absent). The contract
    /// gate itself is tested separately in the `test_contract_gate_*`
    /// tests. This field is `#[cfg(test)]` — it does NOT exist in
    /// production builds.
    #[cfg(test)]
    pub(crate) bypass_contract_gate: bool,
}

impl Actuator {
    pub(crate) fn new(state_dir: PathBuf) -> Self {
        Self::new_with_kernel(state_dir, Box::new(RealKernel::new()))
    }

    /// F2: construct an actuator with an injected `KernelIo`. Used by
    /// fault-injection tests to pass a `FaultKernel`. Production callers
    /// use `new()`, which delegates here with `RealKernel::new()`.
    pub(crate) fn new_with_kernel(state_dir: PathBuf, kernel: Box<dyn KernelIo>) -> Self {
        let log_path = state_dir.join("actions.log");
        let audit_path = state_dir.join("audit.jsonl");
        Self {
            state_dir,
            log_path,
            audit_path,
            pmqos_sink: Box::new(RealPmqosSink::new()),
            last_cpu_latency: None,
            last_device_latencies: HashMap::new(),
            last_runtime_pm: HashMap::new(),
            last_pcie_aspm: HashMap::new(),
            last_sata_alpm: HashMap::new(),
            last_backlight: HashMap::new(),
            allowlist: None,
            boot_state: None,
            active_floors: None,
            kernel,
            correlation_id: String::new(),
            #[cfg(test)]
            fail_nth_runtime_pm_write: None,
            #[cfg(test)]
            bypass_contract_gate: false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn new_with_sink(state_dir: PathBuf, sink: Box<dyn PmqosSink>) -> Self {
        let log_path = state_dir.join("actions.log");
        let audit_path = state_dir.join("audit.jsonl");
        Self {
            state_dir,
            log_path,
            audit_path,
            pmqos_sink: sink,
            last_cpu_latency: None,
            last_device_latencies: HashMap::new(),
            last_runtime_pm: HashMap::new(),
            last_pcie_aspm: HashMap::new(),
            last_sata_alpm: HashMap::new(),
            last_backlight: HashMap::new(),
            allowlist: None,
            boot_state: None,
            active_floors: None,
            kernel: Box::new(RealKernel::new()),
            correlation_id: String::new(),
            #[cfg(test)]
            fail_nth_runtime_pm_write: None,
            #[cfg(test)]
            bypass_contract_gate: false,
        }
    }

    /// Enable the WP-N4 hardware allowlist gate with the given (already loaded)
    /// allowlist. Called from `main` when `--allowlist` is set.
    pub(crate) fn enable_allowlist(&mut self, allowlist: Allowlist) {
        self.allowlist = Some(allowlist);
    }

    /// SPEC §3: install the contract floors for the current tick. `main`
    /// calls this after resolving `committed_class` and before applying
    /// the decision, so the gate always evaluates against the class the
    /// daemon actually committed to this cycle.
    pub(crate) fn set_active_floors(&mut self, floors: ContractFloors) {
        self.active_floors = Some(floors);
    }

    pub(crate) fn set_correlation_id(&mut self, correlation_id: String) {
        self.correlation_id = correlation_id;
    }

    /// SPEC §3 contract gate:
    ///
    /// ```text
    /// exit_latency(S) ≤ active_contract.floor(D)
    /// ```
    ///
    /// Returns `Ok(true)` when the action may proceed. Only the two
    /// depth-enablers that trade resume latency for power are gated:
    ///
    /// - `DeviceResumeLatency` — the written value is a **QoS ceiling
    ///   constraint**, not measured selected-state exit latency. Setting
    ///   unconstrained (`None`) fails closed; a ceiling above the class
    ///   floor is denied.
    /// - `RuntimePm` — autosuspend delay is **not** exit latency and is
    ///   never converted to microseconds for this gate. Without measured
    ///   or hardware-proven [`ExitLatencyEvidence`] (C1), runtime-PM is
    ///   denied with an operator-visible reason.
    ///
    /// Every other variant is ungated and returns `true`.
    ///
    /// **Floor validation (post-#338 review):** the contract gate is
    /// the SPEC §3 rule `exit_latency(S) ≤ active_contract.floor(D)`.
    /// Only the two depth-enablers that trade resume latency for power
    /// are gated: `DeviceResumeLatency` and `RuntimePm`. Every other
    /// variant is ungated.
    ///
    /// Fail-closed semantics:
    /// - **No contract installed** (`active_floors` is `None`): the two
    ///   depth-enablers are DENIED. The previous behavior returned
    ///   `Ok(true)` (gate open), which let a daemon that forgot to call
    ///   `set_active_floors` ship depth-enabling writes with no floor.
    ///   The spec requires "missing, zero or negative contract floors
    ///   must deny depth-enabling actions"; a missing contract is the
    ///   strongest form of "missing floor".
    /// - **Zero or negative floor**: DENY both depth-enablers (malformed
    ///   `contracts.toml`).
    /// - **Valid positive floor**: evaluate `fits_contract` /
    ///   `contract_gate_runtime_pm` against the floor.
    ///
    /// Every denial is logged so the operator can see why a
    /// depth-enabler was blocked and fix the configuration.
    fn contract_gate(&mut self, action: &Action) -> io::Result<GateEvaluation> {
        #[cfg(test)]
        if self.bypass_contract_gate {
            return Ok(GateEvaluation::allowed(
                GateStage::Contract,
                GateReasonCode::ContractAllowed,
            ));
        }

        let label = match action {
            Action::DeviceResumeLatency { .. } | Action::RuntimePm { .. } => {
                action.stable_target_id()
            }
            _ => {
                return Ok(GateEvaluation::not_applicable(
                    GateStage::Contract,
                    GateReasonCode::ContractNotApplicable,
                ));
            }
        };

        let Some(floors) = self.active_floors else {
            self.log(&format!(
                "contract gate BLOCKED {label}: no active contract installed"
            ))?;
            return Ok(GateEvaluation::denied(
                GateStage::Contract,
                GateReasonCode::ContractMissing,
                "no active contract installed",
            ));
        };

        let floor_us: u64 = match u64::try_from(floors.device_resume_latency) {
            Ok(floor) if floor > 0 => floor,
            _ => {
                self.log(&format!(
                    "contract gate BLOCKED {label}: floor={}us is zero/negative",
                    floors.device_resume_latency
                ))?;
                return Ok(GateEvaluation::denied(
                    GateStage::Contract,
                    GateReasonCode::ContractFloorInvalid,
                    format!(
                        "invalid device resume floor {}us",
                        floors.device_resume_latency
                    ),
                ));
            }
        };

        let result = match action {
            Action::DeviceResumeLatency { value, .. } => {
                contract_gate_device_resume_constraint(value.map(i64::from), floor_us, &label)
            }
            Action::RuntimePm { .. } => {
                let _c1_api_surface = (
                    ExitLatencyEvidence::measured_us,
                    ExitLatencyEvidence::hardware_proven_us,
                );
                let evidence: Option<&ExitLatencyEvidence> = None;
                if let Some(evidence) = evidence {
                    let _ = evidence.is_usable();
                }
                contract_gate_runtime_pm(evidence, floor_us, &label)
            }
            _ => unreachable!("non-depth action returned above"),
        };

        match result {
            ContractGateResult::Permit => Ok(GateEvaluation::allowed(
                GateStage::Contract,
                GateReasonCode::ContractAllowed,
            )),
            ContractGateResult::Deny { reason } => {
                self.log(&reason)?;
                Ok(GateEvaluation::denied(
                    GateStage::Contract,
                    GateReasonCode::ContractDenied,
                    reason,
                ))
            }
        }
    }

    /// optid-safety: install the boot-time decision surface. After this call,
    /// `apply()` checks `boot_state.apply_armed` before performing any dynamic
    /// write, and `apply_baseline()` checks `boot_state.baseline_armed` before
    /// applying the curated baseline.
    pub(crate) fn set_boot_state(&mut self, boot_state: BootState) {
        self.boot_state = Some(boot_state);
    }

    /// optid-safety: apply the curated baseline. This is a small, fixed set of
    /// conservative writes that put the system into a known-good state at
    /// startup. It is independent of the per-cycle `Action`s produced by
    /// `Policy::decide_resolved`.
    ///
    /// Currently the curated baseline writes:
    /// - `/proc/sys/vm/swappiness` = 100 (the balanced-mode default; the
    ///   curated baseline uses balanced values for all four modes).
    ///
    /// The curated baseline is gated by `boot_state.baseline_armed`. If
    /// `boot_state` is `None` (the actuator was constructed without
    /// `set_boot_state`), this is a no-op logged as "boot state not set".
    /// If `baseline_armed` is `false` (dry-run), this is a no-op logged as
    /// "baseline disarmed (dry-run)".
    ///
    /// Returns `Ok(())` on success. A failure to write the baseline is
    /// logged but does NOT propagate — the daemon should still start so the
    /// operator can diagnose.
    pub(crate) fn apply_baseline(&mut self) -> io::Result<()> {
        let armed = match self.boot_state.as_ref() {
            None => return Ok(()),
            Some(bs) => bs.baseline_armed,
        };
        if !armed {
            // Dry-run: skip silently. The boot summary in decisions.log
            // already records that baseline_armed=false; logging here would
            // pollute actions.log and break the "dry-run produces no actions"
            // contract that tests rely on.
            return Ok(());
        }

        // Curated baseline write 1: vm.swappiness = 100 (balanced default).
        // The journal + applied marker ensure crash-consistent revert.
        let path = Path::new("/proc/sys/vm/swappiness");
        let key = "vm_swappiness";
        let value = "100";

        // Read current value (best-effort).
        let old_value = self
            .kernel
            .read_to_string(path)
            .ok()
            .unwrap_or_default()
            .trim()
            .to_string();

        // Journal original if not already journaled.
        let orig_file = self.state_dir.join(format!("original_{key}"));
        if !self.kernel.exists(&orig_file) {
            if let Ok(current_val) = self.kernel.read_to_string(path) {
                let _ = atomic_write_state_file_with(&*self.kernel, &orig_file, current_val.trim());
            }
        }

        // Write intended.
        let intended_file = self.state_dir.join(format!("intended_{key}"));
        let _ = atomic_write_state_file_with(&*self.kernel, &intended_file, value);

        // Apply.
        match self.kernel.write(path, value) {
            Ok(_) => {
                mark_applied_with(&*self.kernel, &self.state_dir, key, value);
                self.log(&format!(
                    "baseline: write {} = {value} (was {old_value})",
                    path.display()
                ))?;
            }
            Err(e) => {
                self.log(&format!("baseline: skip {path:?}: write failed: {e}"))?;
            }
        }
        Ok(())
    }

    /// optid-safety: gate dynamic `Action`s on `boot_state.apply_armed`.
    /// Returns `Ok(true)` when the action may proceed, `Ok(false)` when it
    /// must be skipped (with a logged reason), and `Err` on I/O failure
    /// during the log write.
    ///
    /// When `boot_state` is `None` (legacy callers, integration tests), the
    /// gate is open — the actuator behaves as before. This preserves
    /// back-compat for tests that construct an `Actuator` directly without
    /// calling `set_boot_state`.
    fn apply_gate(&mut self) -> io::Result<GateEvaluation> {
        match self.boot_state.as_ref() {
            None => Ok(GateEvaluation::allowed(
                GateStage::ApplyArmed,
                GateReasonCode::ApplyArmed,
            )),
            Some(boot) if boot.apply_armed => Ok(GateEvaluation::allowed(
                GateStage::ApplyArmed,
                GateReasonCode::ApplyArmed,
            )),
            Some(boot) => {
                let detail = format!(
                    "apply_armed=false policy_load_state={} allowlist_load_state={} allowlist_gate={} baseline_armed={}",
                    boot.policy_load_state,
                    boot.allowlist_load_state,
                    boot.allowlist_gate_enabled,
                    boot.baseline_armed,
                );
                self.log(&format!("skip dynamic write: {detail}"))?;
                Ok(GateEvaluation::denied(
                    GateStage::ApplyArmed,
                    GateReasonCode::ApplyDisarmedByBootState,
                    detail,
                ))
            }
        }
    }

    /// The WP-N4 safety gate (SPEC §3 clause 2). Returns `true` when actuation
    /// for `domain` on the device identified by `hwid` is permitted. `hwid` is
    /// resolved by the caller (`None` ⇒ unresolved modalias ⇒ default-deny);
    /// `context_path` is only used for the human-readable log line. When the
    /// gate is disabled this is a no-op that returns `true`. On denial it
    /// appends an audit record and a log line, then returns `false` so the
    /// caller skips the write — default-deny, denial logged with reason.
    fn allowlist_gate(
        &mut self,
        domain: &str,
        hwid: Option<String>,
        requested_state: u32,
        context_path: &Path,
    ) -> io::Result<GateEvaluation> {
        let outcome = match self.allowlist.as_ref() {
            None => None,
            Some(allowlist) => {
                let version = allowlist.version().to_string();
                match hwid {
                    Some(hwid) => Some((
                        hwid.clone(),
                        allowlist.check(domain, &hwid, requested_state),
                        version,
                    )),
                    None => Some((
                        "unknown".to_string(),
                        Verdict::Deny {
                            reason: "hwid_unresolved".to_string(),
                        },
                        version,
                    )),
                }
            }
        };

        let Some((hwid, verdict, version)) = outcome else {
            return Ok(GateEvaluation::not_applicable(
                GateStage::HardwareAllowlist,
                GateReasonCode::AllowlistDisabled,
            ));
        };

        if verdict.is_allow() {
            return Ok(GateEvaluation::allowed(
                GateStage::HardwareAllowlist,
                GateReasonCode::AllowlistAllowed,
            ));
        }
        let reason = verdict.deny_reason().unwrap_or("denied").to_string();
        self.audit_denied(&hwid, domain, requested_state, &reason, &version)?;
        self.log(&format!(
            "deny {domain} on {} ({hwid}): {reason}",
            context_path.display()
        ))?;
        Ok(GateEvaluation::denied(
            GateStage::HardwareAllowlist,
            if hwid == "unknown" {
                GateReasonCode::HwidUnresolved
            } else {
                GateReasonCode::AllowlistDenied
            },
            reason,
        ))
    }

    /// Append a structured denial record to the audit log (JSONL, one object
    /// per line) per docs/research/0006-hw-allowlist-db-design.md §1.2.
    fn audit_denied(
        &mut self,
        hwid: &str,
        domain: &str,
        requested_state: u32,
        reason: &str,
        version: &str,
    ) -> io::Result<()> {
        let line = format!(
            "{{\"ts_unix\":{ts},\"event\":\"actuation_denied\",\"hwid\":\"{hwid}\",\
\"domain\":\"{domain}\",\"requested_state\":{requested_state},\
\"deny_reason\":\"{reason}\",\"allowlist_version\":\"{version}\",\"correlation_id\":\"{correlation_id}\"}}\n",
            ts = self.kernel.now_unix(),
            hwid = json_escape(hwid),
            domain = json_escape(domain),
            requested_state = requested_state,
            reason = json_escape(reason),
            version = json_escape(version),
            correlation_id = json_escape(&self.correlation_id),
        );
        append_log_with(&*self.kernel, &self.audit_path, &line)
    }

    pub(crate) fn apply(&mut self, action: &Action) -> io::Result<ActionOutcome> {
        let mut outcome = ActionOutcome::new(action);
        outcome.gates.push(GateEvaluation::allowed(
            GateStage::DomainMode,
            GateReasonCode::DomainActuate,
        ));

        let apply_gate = self.apply_gate()?;
        let apply_denied = apply_gate.disposition == GateDisposition::Denied;
        outcome.gates.push(apply_gate);
        if apply_denied {
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::ApplyGate,
                "dynamic writes are disarmed".to_string(),
            ));
            return Ok(outcome);
        }

        let contract_gate = self.contract_gate(action)?;
        let contract_denied = contract_gate.disposition == GateDisposition::Denied;
        outcome.gates.push(contract_gate);
        if contract_denied {
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::ContractGate,
                "responsiveness contract denied the action".to_string(),
            ));
            return Ok(outcome);
        }

        match action {
            Action::CpuEpp { value, .. } => {
                let paths = discover_cpu_epp_paths_with(self.kernel.as_ref());
                if paths.is_empty() {
                    self.log("skip cpu.epp: no energy_performance_preference paths")?;
                    outcome.gates.push(GateEvaluation::not_evaluated(
                        GateStage::CapabilityValidation,
                    ));
                    outcome.targets.push(TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::MissingTarget,
                    ));
                    return Ok(outcome);
                }
                for path in paths {
                    let target_id = action.stable_expanded_target_id(&path);
                    match Capability::CpuEpp.validate_target(&path) {
                        Ok(()) => outcome.gates.push(GateEvaluation::allowed(
                            GateStage::CapabilityValidation,
                            GateReasonCode::CapabilityAllowed,
                        )),
                        Err(error) => {
                            outcome.gates.push(GateEvaluation::denied(
                                GateStage::CapabilityValidation,
                                GateReasonCode::CapabilityDenied,
                                error.to_string(),
                            ));
                            self.log(&format!(
                                "skip cpu.epp {}: capability validation failed: {error}",
                                path.display()
                            ))?;
                            let mut target = TargetOutcome::unsupported(
                                target_id,
                                OutcomeReasonCode::UnsupportedTarget,
                            );
                            target.detail = Some(error.to_string());
                            outcome.targets.push(target);
                            continue;
                        }
                    }
                    let old_value = self
                        .kernel
                        .read_to_string(&path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if old_value == *value {
                        outcome.targets.push(TargetOutcome {
                            target_id,
                            pipeline_stage: PipelineStage::Write,
                            support: SupportState::Supported,
                            reason: OutcomeReasonCode::RedundantValue,
                            write_attempted: true,
                            write_outcome: WriteOutcome::Redundant,
                            readback: ReadbackOutcome::Confirmed { value: old_value },
                            ownership: OwnershipState::Optid,
                            pending_restore: RestoreState::NotApplicable,
                            responsible_subsystem: ResponsibleSubsystem::KernelIo,
                            detail: None,
                        });
                        continue;
                    }
                    match self.kernel.write(&path, value) {
                        Ok(()) => {
                            self.log(&format!(
                                "write {} = {value} (was {old_value})",
                                path.display()
                            ))?;
                            let readback =
                                readback_from_result(value, self.kernel.read_to_string(&path));
                            outcome
                                .targets
                                .push(TargetOutcome::applied(target_id, value, readback));
                        }
                        Err(error) => {
                            self.log(&format!(
                                "skip cpu.epp {}: write failed: {error}",
                                path.display()
                            ))?;
                            outcome
                                .targets
                                .push(TargetOutcome::failed(target_id, &error));
                        }
                    }
                }
            }
            Action::PlatformProfile { value, .. } => {
                let path = Path::new("/sys/firmware/acpi/platform_profile");
                if !self.kernel.exists(path) {
                    self.log("skip platform.profile: platform_profile is unavailable")?;
                    outcome.targets.push(TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::MissingTarget,
                    ));
                    return Ok(outcome);
                }
                if let Err(error) = Capability::PlatformProfile.validate_target(path) {
                    outcome.gates.push(GateEvaluation::denied(
                        GateStage::CapabilityValidation,
                        GateReasonCode::CapabilityDenied,
                        error.to_string(),
                    ));
                    self.log(&format!(
                        "skip platform.profile: capability validation failed: {error}"
                    ))?;
                    let mut target = TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::UnsupportedTarget,
                    );
                    target.detail = Some(error.to_string());
                    outcome.targets.push(target);
                    return Ok(outcome);
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let old_value = self
                    .kernel
                    .read_to_string(path)
                    .ok()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match self.kernel.write(path, value) {
                    Ok(()) => {
                        self.log(&format!(
                            "write {} = {value} (was {old_value})",
                            path.display()
                        ))?;
                        let readback =
                            readback_from_result(value, self.kernel.read_to_string(path));
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            value,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!("skip platform.profile: write failed: {error}"))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::SystemdSetProperty {
                unit, properties, ..
            } => {
                outcome.gates.push(GateEvaluation::not_applicable(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let result = Command::new("systemctl")
                    .arg("set-property")
                    .arg("--runtime")
                    .arg(unit)
                    .args(properties)
                    .status();
                match result {
                    Ok(status) if status.success() => {
                        self.log(&format!(
                            "systemctl set-property --runtime {unit} {}",
                            properties.join(" ")
                        ))?;
                        outcome.targets.push(TargetOutcome {
                            target_id: action.stable_target_id(),
                            pipeline_stage: PipelineStage::Write,
                            support: SupportState::NotApplicable,
                            reason: OutcomeReasonCode::WriteApplied,
                            write_attempted: true,
                            write_outcome: WriteOutcome::Applied,
                            readback: ReadbackOutcome::NotPerformed,
                            ownership: OwnershipState::Optid,
                            pending_restore: RestoreState::NotApplicable,
                            responsible_subsystem: ResponsibleSubsystem::Systemd,
                            detail: None,
                        });
                    }
                    Ok(status) => {
                        let error = io::Error::other(format!("systemctl exited with {status}"));
                        self.log(&format!(
                            "skip systemd.set-property {unit}: systemctl exited with {status}"
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                    Err(error) => {
                        self.log(&format!(
                            "skip systemd.set-property {unit}: systemctl unavailable: {error}"
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::VmSysctl { path, value, .. } => {
                let filename = path
                    .file_name()
                    .and_then(|file| file.to_str())
                    .unwrap_or("");
                let key = format!("vm_{filename}");
                if let Err(error) = Capability::VmSysctl.validate_target(path) {
                    outcome.gates.push(GateEvaluation::denied(
                        GateStage::CapabilityValidation,
                        GateReasonCode::CapabilityDenied,
                        error.to_string(),
                    ));
                    self.log(&format!(
                        "skip vm.sysctl {filename}: capability validation failed: {error}"
                    ))?;
                    let mut target = TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::UnsupportedTarget,
                    );
                    target.detail = Some(error.to_string());
                    outcome.targets.push(target);
                    return Ok(outcome);
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let orig_file = self.state_dir.join(format!("original_{key}"));
                let journal_result = if !self.kernel.exists(&orig_file) {
                    self.kernel.read_to_string(path).and_then(|current| {
                        atomic_write_state_file_with(&*self.kernel, &orig_file, current.trim())
                    })
                } else {
                    Ok(())
                };
                outcome.gates.push(match journal_result {
                    Ok(()) => GateEvaluation::allowed(
                        GateStage::RecoveryJournal,
                        GateReasonCode::JournalSucceeded,
                    ),
                    Err(error) => GateEvaluation::denied(
                        GateStage::RecoveryJournal,
                        GateReasonCode::JournalFailed,
                        error.to_string(),
                    ),
                });
                let intended_file = self.state_dir.join(format!("intended_{key}"));
                let _ = atomic_write_state_file_with(&*self.kernel, &intended_file, value);
                let old_value = self
                    .kernel
                    .read_to_string(path)
                    .ok()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match self.kernel.write(path, value) {
                    Ok(()) => {
                        mark_applied_with(&*self.kernel, &self.state_dir, &key, value);
                        self.log(&format!(
                            "write {} = {value} (was {old_value})",
                            path.display()
                        ))?;
                        let readback =
                            readback_from_result(value, self.kernel.read_to_string(path));
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            value,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!("skip vm.sysctl {filename}: write failed: {error}"))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::CpuDmaLatency { value, reason } => {
                let should_apply = self.last_cpu_latency != Some(*value);
                if !should_apply {
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
                        responsible_subsystem: ResponsibleSubsystem::Actuator,
                        detail: None,
                    });
                    return Ok(outcome);
                }
                let old_value = self
                    .pmqos_sink
                    .read_cpu_latency()
                    .unwrap_or_else(|_| "n/a".to_string());
                let value_string = value
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unconstrained".to_string());
                match self.pmqos_sink.write_cpu_latency(*value) {
                    Ok(()) => {
                        self.last_cpu_latency = Some(*value);
                        mark_applied_with(
                            &*self.kernel,
                            &self.state_dir,
                            "cpu_dma_latency",
                            &value_string,
                        );
                        self.log(&format!(
                            "write /dev/cpu_dma_latency = {value_string} (was {old_value}) reason: {reason}"
                        ))?;
                        let readback =
                            readback_from_result(&value_string, self.pmqos_sink.read_cpu_latency());
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            &value_string,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!(
                            "skip /dev/cpu_dma_latency = {value_string}: write failed: {error} reason: {reason}"
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::DeviceResumeLatency {
                path,
                value,
                reason,
                ..
            } => {
                if let Err(error) = Capability::DeviceResumeLatency.validate_target(path) {
                    outcome.gates.push(GateEvaluation::denied(
                        GateStage::CapabilityValidation,
                        GateReasonCode::CapabilityDenied,
                        error.to_string(),
                    ));
                    self.log(&format!(
                        "skip device_resume_latency {}: capability validation failed: {error}",
                        path.display()
                    ))?;
                    let mut target = TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::UnsupportedTarget,
                    );
                    target.detail = Some(error.to_string());
                    outcome.targets.push(target);
                    return Ok(outcome);
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let allowlist =
                    self.allowlist_gate("runtime_pm", hwid_from_attr_path(path), 0, path)?;
                let denied = allowlist.disposition == GateDisposition::Denied;
                outcome.gates.push(allowlist);
                if denied {
                    outcome.targets.push(TargetOutcome::denied(
                        action.stable_target_id(),
                        PipelineStage::AllowlistGate,
                        "hardware allowlist denied target".to_string(),
                    ));
                    return Ok(outcome);
                }
                if self.last_device_latencies.get(path) == Some(value) {
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
                        responsible_subsystem: ResponsibleSubsystem::Actuator,
                        detail: None,
                    });
                    return Ok(outcome);
                }
                let hash = get_path_hash(path);
                let key = format!("dev_{hash}");
                let orig_file = self.state_dir.join(format!("original_{key}"));
                let journal_result = if !self.kernel.exists(&orig_file) {
                    self.pmqos_sink
                        .read_device_latency(path)
                        .and_then(|current| {
                            atomic_write_state_file_with(
                                &*self.kernel,
                                &orig_file,
                                &format!("{}\n{}", path.display(), current.trim()),
                            )
                        })
                } else {
                    Ok(())
                };
                outcome.gates.push(match journal_result {
                    Ok(()) => GateEvaluation::allowed(
                        GateStage::RecoveryJournal,
                        GateReasonCode::JournalSucceeded,
                    ),
                    Err(error) => GateEvaluation::denied(
                        GateStage::RecoveryJournal,
                        GateReasonCode::JournalFailed,
                        error.to_string(),
                    ),
                });
                let value_string = value
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "0".to_string());
                let intended_file = self.state_dir.join(format!("intended_{key}"));
                let _ = atomic_write_state_file_with(&*self.kernel, &intended_file, &value_string);
                let old_value = self
                    .pmqos_sink
                    .read_device_latency(path)
                    .ok()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match self.pmqos_sink.write_device_latency(path, &value_string) {
                    Ok(()) => {
                        self.last_device_latencies.insert(path.clone(), *value);
                        mark_applied_with(&*self.kernel, &self.state_dir, &key, &value_string);
                        self.log(&format!(
                            "write {} = {value_string} (was {old_value}) reason: {reason}",
                            path.display()
                        ))?;
                        let readback = readback_from_result(
                            &value_string,
                            self.pmqos_sink.read_device_latency(path),
                        );
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            &value_string,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!(
                            "skip device latency {}: write failed: {error}",
                            path.display()
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::RuntimePm {
                device_dir,
                autosuspend_delay_ms,
                reason,
            } => {
                let control_path = device_dir.join("power").join("control");
                let delay_path = device_dir.join("power").join("autosuspend_delay_ms");
                for target in [&control_path, &delay_path] {
                    if target == &delay_path && !self.kernel.exists(target) {
                        continue;
                    }
                    if let Err(error) = Capability::RuntimePm.validate_target(target) {
                        outcome.gates.push(GateEvaluation::denied(
                            GateStage::CapabilityValidation,
                            GateReasonCode::CapabilityDenied,
                            error.to_string(),
                        ));
                        self.log(&format!(
                            "skip runtime_pm {}: capability validation failed: {error}",
                            device_dir.display()
                        ))?;
                        let mut target_outcome = TargetOutcome::unsupported(
                            action.stable_target_id(),
                            OutcomeReasonCode::UnsupportedTarget,
                        );
                        target_outcome.detail = Some(error.to_string());
                        outcome.targets.push(target_outcome);
                        return Ok(outcome);
                    }
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let allowlist = self.allowlist_gate(
                    "runtime_pm",
                    hwid_from_device_dir(device_dir),
                    0,
                    device_dir,
                )?;
                let denied = allowlist.disposition == GateDisposition::Denied;
                outcome.gates.push(allowlist);
                if denied {
                    outcome.targets.push(TargetOutcome::denied(
                        action.stable_target_id(),
                        PipelineStage::AllowlistGate,
                        "hardware allowlist denied target".to_string(),
                    ));
                    return Ok(outcome);
                }
                if runtime_pm::network_carrier_up(device_dir) {
                    self.log(&format!(
                        "skip runtime_pm {}: network carrier up",
                        device_dir.display()
                    ))?;
                    outcome.targets.push(TargetOutcome {
                        target_id: action.stable_target_id(),
                        pipeline_stage: PipelineStage::Write,
                        support: SupportState::Supported,
                        reason: OutcomeReasonCode::NetworkCarrierUp,
                        write_attempted: false,
                        write_outcome: WriteOutcome::Skipped,
                        readback: ReadbackOutcome::NotPerformed,
                        ownership: OwnershipState::Unowned,
                        pending_restore: RestoreState::NotApplicable,
                        responsible_subsystem: ResponsibleSubsystem::Actuator,
                        detail: Some("network carrier is up".to_string()),
                    });
                    return Ok(outcome);
                }
                if let Some(warning) = runtime_pm::wakeup_warning(device_dir) {
                    self.log(&format!("warn runtime_pm: {warning}"))?;
                }
                if self.last_runtime_pm.get(device_dir) == Some(autosuspend_delay_ms) {
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
                        responsible_subsystem: ResponsibleSubsystem::Actuator,
                        detail: None,
                    });
                    return Ok(outcome);
                }
                let delay_string = autosuspend_delay_ms.to_string();
                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_rpm_{hash}"));
                let intended_file = self.state_dir.join(format!("intended_rpm_{hash}"));
                let (orig_control, orig_delay) = if self.kernel.exists(&orig_file) {
                    let content = self.kernel.read_to_string(&orig_file).unwrap_or_default();
                    let mut lines = content.lines();
                    let _ = lines.next();
                    (
                        lines.next().unwrap_or("on").to_string(),
                        lines.next().unwrap_or("n/a").trim().to_string(),
                    )
                } else {
                    (
                        self.kernel
                            .read_to_string(&control_path)
                            .ok()
                            .map(|value| value.trim().to_string())
                            .unwrap_or_else(|| "on".to_string()),
                        if self.kernel.exists(&delay_path) {
                            self.kernel
                                .read_to_string(&delay_path)
                                .ok()
                                .map(|value| value.trim().to_string())
                                .unwrap_or_else(|| "n/a".to_string())
                        } else {
                            "n/a".to_string()
                        },
                    )
                };
                let journal = atomic_write_state_file_with(
                    &*self.kernel,
                    &orig_file,
                    &format!("{}\n{orig_control}\n{orig_delay}", device_dir.display()),
                );
                match journal {
                    Ok(()) => outcome.gates.push(GateEvaluation::allowed(
                        GateStage::RecoveryJournal,
                        GateReasonCode::JournalSucceeded,
                    )),
                    Err(error) => {
                        outcome.gates.push(GateEvaluation::denied(
                            GateStage::RecoveryJournal,
                            GateReasonCode::JournalFailed,
                            error.to_string(),
                        ));
                        self.log(&format!(
                            "skip runtime_pm {}: failed to write recovery journal: {error}",
                            device_dir.display()
                        ))?;
                        outcome.targets.push(TargetOutcome::denied(
                            action.stable_target_id(),
                            PipelineStage::Journal,
                            "recovery journal failed".to_string(),
                        ));
                        return Ok(outcome);
                    }
                }
                let _ = atomic_write_state_file_with(
                    &*self.kernel,
                    &intended_file,
                    &format!("auto\n{delay_string}"),
                );
                if self.kernel.exists(&delay_path) {
                    if let Err(error) = self.runtime_pm_write(&delay_path, &delay_string, 1) {
                        self.log(&format!(
                            "skip runtime_pm delay {}: write failed (no rollback needed): {error}",
                            device_dir.display()
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                        return Ok(outcome);
                    }
                }
                match self.runtime_pm_write(&control_path, "auto", 2) {
                    Ok(()) => {
                        self.last_runtime_pm
                            .insert(device_dir.clone(), *autosuspend_delay_ms);
                        mark_applied_with(
                            &*self.kernel,
                            &self.state_dir,
                            &format!("rpm_{hash}"),
                            &format!("auto\n{delay_string}"),
                        );
                        self.log(&format!(
                            "write {} control=auto autosuspend_delay_ms={delay_string} reason: {reason}",
                            device_dir.display()
                        ))?;
                        let control_readback =
                            readback_from_result("auto", self.kernel.read_to_string(&control_path));
                        let mut target = TargetOutcome::applied(
                            action.stable_target_id(),
                            "auto",
                            control_readback,
                        );
                        if self.kernel.exists(&delay_path) {
                            let delay_readback = readback_from_result(
                                &delay_string,
                                self.kernel.read_to_string(&delay_path),
                            );
                            if matches!(delay_readback, ReadbackOutcome::Mismatch { .. }) {
                                target.reason = OutcomeReasonCode::ReadbackMismatch;
                                target.ownership = OwnershipState::Drifted;
                                target.readback = delay_readback;
                            }
                        }
                        outcome.targets.push(target);
                    }
                    Err(error) => {
                        self.log(&format!(
                            "runtime_pm {}: control write failed after delay write succeeded; rolling back delay: {error}",
                            device_dir.display()
                        ))?;
                        let mut target = TargetOutcome::failed(action.stable_target_id(), &error);
                        if self.kernel.exists(&delay_path) && orig_delay != "n/a" {
                            match self.runtime_pm_write(&delay_path, &orig_delay, 3) {
                                Ok(()) => {
                                    self.log(&format!(
                                        "runtime_pm {}: rolled back delay to {orig_delay}",
                                        device_dir.display()
                                    ))?;
                                    target.pending_restore = RestoreState::Restored;
                                }
                                Err(rollback_error) => {
                                    self.log(&format!(
                                        "runtime_pm {}: ROLLBACK FAILED — journal retained: {rollback_error}",
                                        device_dir.display()
                                    ))?;
                                    target.pending_restore = RestoreState::Pending;
                                    target.detail = Some(format!(
                                        "control write failed: {error}; rollback failed: {rollback_error}"
                                    ));
                                }
                            }
                        }
                        outcome.targets.push(target);
                    }
                }
            }
            Action::PcieAspm {
                device_dir,
                enable,
                reason,
            } => {
                let target_path = device_dir.join("link").join("l1_aspm");
                if let Err(error) = Capability::PcieAspm.validate_target(&target_path) {
                    outcome.gates.push(GateEvaluation::denied(
                        GateStage::CapabilityValidation,
                        GateReasonCode::CapabilityDenied,
                        error.to_string(),
                    ));
                    self.log(&format!(
                        "skip pcie_aspm {}: capability validation failed: {error}",
                        device_dir.display()
                    ))?;
                    let mut target = TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::UnsupportedTarget,
                    );
                    target.detail = Some(error.to_string());
                    outcome.targets.push(target);
                    return Ok(outcome);
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let allowlist = self.allowlist_gate(
                    "pci_aspm",
                    hwid_from_device_dir(device_dir),
                    0,
                    device_dir,
                )?;
                let denied = allowlist.disposition == GateDisposition::Denied;
                outcome.gates.push(allowlist);
                if denied {
                    outcome.targets.push(TargetOutcome::denied(
                        action.stable_target_id(),
                        PipelineStage::AllowlistGate,
                        "hardware allowlist denied target".to_string(),
                    ));
                    return Ok(outcome);
                }
                if storage::is_cnvi(device_dir) {
                    self.log(&format!(
                        "skip pcie_aspm {}: CNVi device (link PM is firmware-managed)",
                        device_dir.display()
                    ))?;
                    outcome.targets.push(TargetOutcome {
                        target_id: action.stable_target_id(),
                        pipeline_stage: PipelineStage::Write,
                        support: SupportState::NotApplicable,
                        reason: OutcomeReasonCode::NotApplicable,
                        write_attempted: false,
                        write_outcome: WriteOutcome::NotApplicable,
                        readback: ReadbackOutcome::NotPerformed,
                        ownership: OwnershipState::Unowned,
                        pending_restore: RestoreState::NotApplicable,
                        responsible_subsystem: ResponsibleSubsystem::Actuator,
                        detail: Some("CNVi link power is firmware-managed".to_string()),
                    });
                    return Ok(outcome);
                }
                if self.last_pcie_aspm.get(device_dir) == Some(enable) {
                    outcome.targets.push(redundant_target(action));
                    return Ok(outcome);
                }
                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_aspm_{hash}"));
                if !self.kernel.exists(&orig_file) {
                    let original = self
                        .kernel
                        .read_to_string(&target_path)
                        .ok()
                        .map(|value| value.trim().to_string())
                        .unwrap_or_else(|| "0".to_string());
                    let _ = atomic_write_state_file_with(
                        &*self.kernel,
                        &orig_file,
                        &format!("{}\n{original}", device_dir.display()),
                    );
                }
                let value = if *enable { "1" } else { "0" };
                let intended_file = self.state_dir.join(format!("intended_aspm_{hash}"));
                let _ = atomic_write_state_file_with(&*self.kernel, &intended_file, value);
                match self.kernel.write(&target_path, value) {
                    Ok(()) => {
                        self.last_pcie_aspm.insert(device_dir.clone(), *enable);
                        mark_applied_with(
                            &*self.kernel,
                            &self.state_dir,
                            &format!("aspm_{hash}"),
                            value,
                        );
                        self.log(&format!(
                            "write {} l1_aspm={value} reason: {reason}",
                            target_path.display()
                        ))?;
                        let readback =
                            readback_from_result(value, self.kernel.read_to_string(&target_path));
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            value,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!(
                            "skip pcie_aspm {}: write failed: {error}",
                            device_dir.display()
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::SataAlpm {
                host_dir,
                policy,
                reason,
            } => {
                let target_path = host_dir.join("link_power_management_policy");
                if let Err(error) = Capability::SataAlpm.validate_target(&target_path) {
                    outcome.gates.push(GateEvaluation::denied(
                        GateStage::CapabilityValidation,
                        GateReasonCode::CapabilityDenied,
                        error.to_string(),
                    ));
                    self.log(&format!(
                        "skip sata_alpm {}: capability validation failed: {error}",
                        host_dir.display()
                    ))?;
                    let mut target = TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::UnsupportedTarget,
                    );
                    target.detail = Some(error.to_string());
                    outcome.targets.push(target);
                    return Ok(outcome);
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let allowlist =
                    self.allowlist_gate("sata_alpm", hwid_from_ancestors(host_dir), 0, host_dir)?;
                let denied = allowlist.disposition == GateDisposition::Denied;
                outcome.gates.push(allowlist);
                if denied {
                    outcome.targets.push(TargetOutcome::denied(
                        action.stable_target_id(),
                        PipelineStage::AllowlistGate,
                        "hardware allowlist denied target".to_string(),
                    ));
                    return Ok(outcome);
                }
                if self.last_sata_alpm.get(host_dir).map(String::as_str) == Some(policy.as_str()) {
                    outcome.targets.push(redundant_target(action));
                    return Ok(outcome);
                }
                let hash = get_path_hash(host_dir);
                let orig_file = self.state_dir.join(format!("original_alpm_{hash}"));
                if !self.kernel.exists(&orig_file) {
                    let original = self
                        .kernel
                        .read_to_string(&target_path)
                        .ok()
                        .map(|value| value.trim().to_string())
                        .unwrap_or_else(|| "max_performance".to_string());
                    let _ = atomic_write_state_file_with(
                        &*self.kernel,
                        &orig_file,
                        &format!("{}\n{original}", host_dir.display()),
                    );
                }
                let intended_file = self.state_dir.join(format!("intended_alpm_{hash}"));
                let _ = atomic_write_state_file_with(&*self.kernel, &intended_file, policy);
                match self.kernel.write(&target_path, policy) {
                    Ok(()) => {
                        self.last_sata_alpm.insert(host_dir.clone(), policy.clone());
                        mark_applied_with(
                            &*self.kernel,
                            &self.state_dir,
                            &format!("alpm_{hash}"),
                            policy,
                        );
                        self.log(&format!(
                            "write {} policy={policy} reason: {reason}",
                            target_path.display()
                        ))?;
                        let readback =
                            readback_from_result(policy, self.kernel.read_to_string(&target_path));
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            policy,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!(
                            "skip sata_alpm {}: write failed: {error}",
                            host_dir.display()
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
            Action::Backlight {
                device_dir,
                target_pct,
                reason,
            } => {
                let target_path = device_dir.join("brightness");
                if let Err(error) = Capability::Backlight.validate_target(&target_path) {
                    outcome.gates.push(GateEvaluation::denied(
                        GateStage::CapabilityValidation,
                        GateReasonCode::CapabilityDenied,
                        error.to_string(),
                    ));
                    self.log(&format!(
                        "skip backlight {}: capability validation failed: {error}",
                        device_dir.display()
                    ))?;
                    let mut target = TargetOutcome::unsupported(
                        action.stable_target_id(),
                        OutcomeReasonCode::UnsupportedTarget,
                    );
                    target.detail = Some(error.to_string());
                    outcome.targets.push(target);
                    return Ok(outcome);
                }
                outcome.gates.push(GateEvaluation::allowed(
                    GateStage::CapabilityValidation,
                    GateReasonCode::CapabilityAllowed,
                ));
                let allowlist = self.allowlist_gate(
                    "backlight",
                    hwid_from_ancestors(device_dir),
                    0,
                    device_dir,
                )?;
                let denied = allowlist.disposition == GateDisposition::Denied;
                outcome.gates.push(allowlist);
                if denied {
                    outcome.targets.push(TargetOutcome::denied(
                        action.stable_target_id(),
                        PipelineStage::AllowlistGate,
                        "hardware allowlist denied target".to_string(),
                    ));
                    return Ok(outcome);
                }
                let max = match display::read_max_brightness(device_dir) {
                    Some(max) if max > 0 => max,
                    _ => {
                        self.log(&format!(
                            "skip backlight {}: no usable max_brightness",
                            device_dir.display()
                        ))?;
                        outcome.targets.push(TargetOutcome::unsupported(
                            action.stable_target_id(),
                            OutcomeReasonCode::MissingTarget,
                        ));
                        return Ok(outcome);
                    }
                };
                let target = display::compute_target_brightness(max, *target_pct);
                if self.last_backlight.get(device_dir) == Some(&target) {
                    outcome.targets.push(redundant_target(action));
                    return Ok(outcome);
                }
                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_bl_{hash}"));
                if !self.kernel.exists(&orig_file) {
                    let original = self
                        .kernel
                        .read_to_string(&target_path)
                        .ok()
                        .map(|value| value.trim().to_string())
                        .unwrap_or_default();
                    let _ = atomic_write_state_file_with(
                        &*self.kernel,
                        &orig_file,
                        &format!("{}\n{original}", device_dir.display()),
                    );
                }
                let target_string = target.to_string();
                let intended_file = self.state_dir.join(format!("intended_bl_{hash}"));
                let _ = atomic_write_state_file_with(&*self.kernel, &intended_file, &target_string);
                match self.kernel.write(&target_path, &target_string) {
                    Ok(()) => {
                        self.last_backlight.insert(device_dir.clone(), target);
                        mark_applied_with(
                            &*self.kernel,
                            &self.state_dir,
                            &format!("bl_{hash}"),
                            &target_string,
                        );
                        self.log(&format!(
                            "write {} brightness={target_string} (target {target_pct}% of {max}) reason: {reason}",
                            target_path.display()
                        ))?;
                        let readback = readback_from_result(
                            &target_string,
                            self.kernel.read_to_string(&target_path),
                        );
                        outcome.targets.push(TargetOutcome::applied(
                            action.stable_target_id(),
                            &target_string,
                            readback,
                        ));
                    }
                    Err(error) => {
                        self.log(&format!(
                            "skip backlight {}: write failed: {error}",
                            device_dir.display()
                        ))?;
                        outcome
                            .targets
                            .push(TargetOutcome::failed(action.stable_target_id(), &error));
                    }
                }
            }
        }
        Ok(outcome)
    }

    /// Revert a single journaled action, identified by the journal key
    /// that `Action::journal_key()` derives.
    ///
    /// This is the inverse-restore path for a *context change*: the
    /// previous decision applied `key`, the new decision no longer
    /// contains it, so the value must go back to its journaled original
    /// now rather than lingering until shutdown. Before this existed, a
    /// battery→AC transition left battery-idle sysfs values in place for
    /// the rest of the uptime.
    ///
    /// Journal formats are the ones `apply` writes, and match the
    /// shutdown reverts in `io_util`:
    ///
    /// - `rpm_<hash>`: three lines — device dir, original `power/control`,
    ///   original `power/autosuspend_delay_ms` (or the literal `n/a`).
    /// - `dev_<hash>`: two lines — attribute path, original value.
    /// - `aspm_<hash>` / `alpm_<hash>` / `bl_<hash>`: two lines — base
    ///   directory, original value.
    ///
    /// `cpu_epp`, `platform_profile`, `vm_*` and `cpu_dma_latency` are
    /// deliberately **not** handled here. They are system-wide knobs that
    /// every decision tick rewrites unconditionally, so the next tick
    /// already overwrites them; adding a per-key restoration mechanism
    /// would fight that loop and could bounce a value the new decision is
    /// about to set. They keep their existing shutdown revert.
    ///
    /// Returns `Ok(true)` when a restoration ran and the journal was
    /// cleared, `Ok(false)` when there was nothing to do (unknown or
    /// non-revertible key, or no journal on disk). On a failed write the
    /// journal is **retained** so the shutdown revert can retry, and
    /// `Ok(false)` is returned.
    pub(crate) fn revert_key_outcome(&mut self, key: &str) -> io::Result<RestoreOutcome> {
        let journal = self.state_dir.join(format!("original_{key}"));
        let existed = self.kernel.exists(&journal);
        let restored = self.revert_key(key)?;
        if restored {
            return Ok(RestoreOutcome {
                target_id: format!("journal:{key}"),
                pipeline_stage: PipelineStage::Restore,
                reason: OutcomeReasonCode::RestoreApplied,
                write_attempted: true,
                write_outcome: WriteOutcome::Restored,
                readback: ReadbackOutcome::NotPerformed,
                ownership: OwnershipState::Unowned,
                pending_restore: RestoreState::Restored,
                responsible_subsystem: ResponsibleSubsystem::Restoration,
                detail: None,
            });
        }
        if existed && self.kernel.exists(&journal) {
            return Ok(RestoreOutcome {
                target_id: format!("journal:{key}"),
                pipeline_stage: PipelineStage::Restore,
                reason: OutcomeReasonCode::RestoreFailed,
                write_attempted: true,
                write_outcome: WriteOutcome::RestorationFailed {
                    error_kind: ErrorKindCode::Other,
                },
                readback: ReadbackOutcome::NotPerformed,
                ownership: OwnershipState::Optid,
                pending_restore: RestoreState::Pending,
                responsible_subsystem: ResponsibleSubsystem::Restoration,
                detail: Some("restore did not complete; journal retained".to_string()),
            });
        }
        Ok(RestoreOutcome {
            target_id: format!("journal:{key}"),
            pipeline_stage: PipelineStage::Restore,
            reason: OutcomeReasonCode::NotApplicable,
            write_attempted: false,
            write_outcome: WriteOutcome::NotApplicable,
            readback: ReadbackOutcome::NotPerformed,
            ownership: OwnershipState::Relinquished,
            pending_restore: RestoreState::NotApplicable,
            responsible_subsystem: ResponsibleSubsystem::Restoration,
            detail: Some("no supported current inverse-restoration journal".to_string()),
        })
    }

    pub(crate) fn revert_key(&mut self, key: &str) -> io::Result<bool> {
        let orig_file = self.state_dir.join(format!("original_{key}"));
        if !self.kernel.exists(&orig_file) {
            return Ok(false);
        }
        let Ok(content) = self.kernel.read_to_string(&orig_file) else {
            return Ok(false);
        };
        let mut lines = content.lines();

        let restored = if key.starts_with("rpm_") {
            // Three-line journal: device dir, control, delay.
            let (Some(dev_dir), Some(orig_control)) = (lines.next(), lines.next()) else {
                return Ok(false);
            };
            let orig_delay = lines.next().unwrap_or("n/a").trim().to_string();
            let dev_dir = PathBuf::from(dev_dir);
            let control_path = dev_dir.join("power").join("control");
            let orig_control = orig_control.trim().to_string();

            match self.kernel.write(&control_path, &orig_control) {
                Ok(()) => {
                    // Restore the delay too, when the device had one.
                    let mut ok = true;
                    if orig_delay != "n/a" {
                        let delay_path = dev_dir.join("power").join("autosuspend_delay_ms");
                        if let Err(e) = self.kernel.write(&delay_path, &orig_delay) {
                            self.log(&format!(
                                "context-change revert {key}: failed to restore autosuspend_delay_ms for {}: {e}",
                                dev_dir.display()
                            ))?;
                            ok = false;
                        }
                    }
                    if ok {
                        self.last_runtime_pm.remove(&dev_dir);
                        self.log(&format!(
                            "context-change revert {key}: restored {} control={orig_control} autosuspend_delay_ms={orig_delay}",
                            dev_dir.display()
                        ))?;
                    }
                    ok
                }
                Err(e) => {
                    self.log(&format!(
                        "context-change revert {key}: failed to restore control for {}: {e}",
                        dev_dir.display()
                    ))?;
                    false
                }
            }
        } else if key.starts_with("dev_") {
            // Two-line journal: attribute path, original value.
            let (Some(attr_path), Some(orig_val)) = (lines.next(), lines.next()) else {
                return Ok(false);
            };
            let attr_path = PathBuf::from(attr_path);
            let orig_val = orig_val.trim().to_string();
            // DeviceResumeLatency applies through the PM QoS sink, so the
            // restore must go back through the same sink — not the raw
            // kernel writer — or a mocked sink would diverge from what the
            // apply path actually mutated.
            match self.pmqos_sink.write_device_latency(&attr_path, &orig_val) {
                Ok(()) => {
                    self.last_device_latencies.remove(&attr_path);
                    self.log(&format!(
                        "context-change revert {key}: restored {} = {orig_val}",
                        attr_path.display()
                    ))?;
                    true
                }
                Err(e) => {
                    self.log(&format!(
                        "context-change revert {key}: failed to restore {}: {e}",
                        attr_path.display()
                    ))?;
                    false
                }
            }
        } else if key.starts_with("aspm_") || key.starts_with("alpm_") || key.starts_with("bl_") {
            // Two-line journal: base directory, original value. Only the
            // attribute path relative to that base differs per domain.
            let (Some(base), Some(orig_val)) = (lines.next(), lines.next()) else {
                return Ok(false);
            };
            let base = PathBuf::from(base);
            let orig_val = orig_val.trim().to_string();
            let target = if key.starts_with("aspm_") {
                base.join("link").join("l1_aspm")
            } else if key.starts_with("alpm_") {
                base.join("link_power_management_policy")
            } else {
                base.join("brightness")
            };

            match self.kernel.write(&target, &orig_val) {
                Ok(()) => {
                    if key.starts_with("aspm_") {
                        self.last_pcie_aspm.remove(&base);
                    } else if key.starts_with("alpm_") {
                        self.last_sata_alpm.remove(&base);
                    } else {
                        self.last_backlight.remove(&base);
                    }
                    self.log(&format!(
                        "context-change revert {key}: restored {} = {orig_val}",
                        target.display()
                    ))?;
                    true
                }
                Err(e) => {
                    self.log(&format!(
                        "context-change revert {key}: failed to restore {}: {e}",
                        target.display()
                    ))?;
                    false
                }
            }
        } else {
            // System-wide knobs (cpu_epp, platform_profile, vm_*,
            // cpu_dma_latency) are continuously overwritten by later
            // ticks; nothing to do here.
            return Ok(false);
        };

        if restored {
            // Drop original_/intended_/applied_ together.
            clear_journal_with(&*self.kernel, &self.state_dir, key);
        } else {
            self.log(&format!(
                "context-change revert {key}: journal retained; restore did not complete"
            ))?;
        }
        Ok(restored)
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        let correlation_id = if self.correlation_id.is_empty() {
            "unscoped"
        } else {
            &self.correlation_id
        };
        append_log_with(
            &*self.kernel,
            &self.log_path,
            &format!(
                "{} correlation_id={} {message}\n",
                self.kernel.now_unix(),
                correlation_id
            ),
        )
    }

    /// Phase 6: write helper for RuntimePm's transactional two-write + rollback
    /// sequence. In production this is a thin wrapper around `guarded_write`.
    /// In test builds, if `fail_nth_runtime_pm_write` is `Some(n)` and `n`
    /// matches `write_num`, a synthetic `Err` is returned instead. This keeps
    /// the test-hook state `#[cfg(test)]`-only (zero production overhead)
    /// while giving tests deterministic control over each failure point.
    fn runtime_pm_write(&mut self, path: &Path, value: &str, write_num: usize) -> io::Result<()> {
        #[cfg(test)]
        {
            if self.fail_nth_runtime_pm_write == Some(write_num) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "test-injected failure on RuntimePm write #{} ({})",
                        write_num,
                        match write_num {
                            1 => "delay",
                            2 => "control",
                            3 => "rollback",
                            _ => "unknown",
                        }
                    ),
                ));
            }
        }
        #[cfg(not(test))]
        {
            let _ = write_num; // suppress unused-variable warning in production
        }
        self.kernel.write(path, value)
    }
}

fn redundant_target(action: &Action) -> TargetOutcome {
    TargetOutcome {
        target_id: action.stable_target_id(),
        pipeline_stage: PipelineStage::Write,
        support: SupportState::Supported,
        reason: OutcomeReasonCode::RedundantValue,
        write_attempted: false,
        write_outcome: WriteOutcome::Redundant,
        readback: ReadbackOutcome::NotPerformed,
        ownership: OwnershipState::Optid,
        pending_restore: RestoreState::Pending,
        responsible_subsystem: ResponsibleSubsystem::Actuator,
        detail: None,
    }
}

/// Minimal JSON string escaping for audit records. The fields are controlled
/// (HWIDs, domain names, reason codes) but a stray quote/backslash must never
/// corrupt the JSONL audit stream.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
