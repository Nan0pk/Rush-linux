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
use crate::contracts::{fits_contract, ContractFloors};
use crate::io_util::{
    append_log, atomic_write_state_file, clear_journal, get_path_hash, mark_applied,
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
    /// Test-only hook: when `Some(n)`, the `n`-th `guarded_write` call within
    /// a single `Action::RuntimePm` apply (1 = delay write, 2 = control write,
    /// 3 = rollback delay write) returns a synthetic `Err`. This field is
    /// `#[cfg(test)]` — it does NOT exist in production builds, so there is
    /// zero test-hook state in the production binary.
    #[cfg(test)]
    pub(crate) fail_nth_runtime_pm_write: Option<usize>,
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
            #[cfg(test)]
            fail_nth_runtime_pm_write: None,
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
            #[cfg(test)]
            fail_nth_runtime_pm_write: None,
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

    /// SPEC §3 contract gate:
    ///
    /// ```text
    /// exit_latency(S) ≤ active_contract.floor(D)
    /// ```
    ///
    /// Returns `Ok(true)` when the action may proceed. Only the two
    /// depth-enablers that trade resume latency for power are gated:
    ///
    /// - `DeviceResumeLatency` — the action value *is* the resume latency
    ///   in microseconds. `None` means "no constraint" (the PM QoS default),
    ///   which cannot violate a floor.
    /// - `RuntimePm` — autosuspend delay is expressed in milliseconds, so
    ///   the exit latency is `autosuspend_delay_ms × 1000` µs.
    ///
    /// Every other variant is ungated and returns `true`: CPU EPP,
    /// platform profile and cgroup weights do not change a device's exit
    /// latency, and CPU DMA latency is itself a latency *floor* request
    /// rather than a state with an exit cost.
    ///
    /// Negative values are treated as "no constraint" rather than being
    /// cast to `u64`, where `-1` would wrap to `u64::MAX` and be blocked
    /// by every floor, or a negative delay would wrap into a small
    /// apparently-valid latency. `-1` is the kernel's own sentinel for an
    /// unset `autosuspend_delay_ms`.
    fn contract_permits(&mut self, action: &Action) -> io::Result<bool> {
        let Some(floors) = self.active_floors else {
            return Ok(true); // gate not installed
        };
        // A non-positive floor cannot be satisfied by any real state and
        // most likely means a misconfigured contracts.toml; treat it as
        // "no contract" rather than blocking all depth-enablers.
        let floor_us: u64 = match u64::try_from(floors.device_resume_latency) {
            Ok(f) if f > 0 => f,
            _ => return Ok(true),
        };

        let (exit_latency_us, label) = match action {
            Action::DeviceResumeLatency { path, value, .. } => match value {
                // No PM QoS constraint requested ⇒ nothing to gate.
                None => return Ok(true),
                Some(v) => match u64::try_from(*v) {
                    Ok(us) => (us, format!("device_resume_latency {}", path.display())),
                    // Negative ⇒ not a real latency; leave it to the
                    // capability/allowlist layers rather than wrapping.
                    Err(_) => return Ok(true),
                },
            },
            Action::RuntimePm {
                device_dir,
                autosuspend_delay_ms,
                ..
            } => {
                // -1 is the kernel sentinel for "unset"; negative values
                // are not a latency budget.
                let Ok(delay_ms) = u64::try_from(*autosuspend_delay_ms) else {
                    return Ok(true);
                };
                // Saturate rather than overflow on an absurd delay.
                let us = delay_ms.saturating_mul(1000);
                (us, format!("runtime_pm {}", device_dir.display()))
            }
            _ => return Ok(true),
        };

        if fits_contract(exit_latency_us, floor_us) {
            return Ok(true);
        }
        self.log(&format!(
            "contract gate BLOCKED {label}: exit_latency={exit_latency_us}us > floor={floor_us}us"
        ))?;
        Ok(false)
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
        if !orig_file.exists() {
            if let Ok(current_val) = self.kernel.read_to_string(path) {
                let _ = atomic_write_state_file(&orig_file, current_val.trim());
            }
        }

        // Write intended.
        let intended_file = self.state_dir.join(format!("intended_{key}"));
        let _ = atomic_write_state_file(&intended_file, value);

        // Apply.
        match self.kernel.write(path, value) {
            Ok(_) => {
                mark_applied(&self.state_dir, key, value);
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
    fn dynamic_writes_armed(&mut self) -> io::Result<bool> {
        match self.boot_state.as_ref() {
            None => Ok(true),
            Some(bs) => {
                if bs.apply_armed {
                    Ok(true)
                } else {
                    self.log(&format!(
                        "skip dynamic write: apply_armed=false \
                         (policy_load_state={} allowlist_load_state={} allowlist_gate={} baseline_armed={})",
                        bs.policy_load_state,
                        bs.allowlist_load_state,
                        bs.allowlist_gate_enabled,
                        bs.baseline_armed,
                    ))?;
                    Ok(false)
                }
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
    fn allowlist_permits(
        &mut self,
        domain: &str,
        hwid: Option<String>,
        requested_state: u32,
        context_path: &Path,
    ) -> io::Result<bool> {
        // Resolve the verdict while only borrowing the allowlist, so the
        // subsequent &mut self logging calls don't conflict with the borrow.
        let outcome = match self.allowlist.as_ref() {
            None => None,
            Some(al) => {
                let version = al.version().to_string();
                match hwid {
                    Some(hwid) => {
                        let verdict = al.check(domain, &hwid, requested_state);
                        Some((hwid, verdict, version))
                    }
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
            return Ok(true); // gate disabled
        };

        if verdict.is_allow() {
            return Ok(true);
        }
        let reason = verdict.deny_reason().unwrap_or("denied").to_string();
        self.audit_denied(&hwid, domain, requested_state, &reason, &version)?;
        self.log(&format!(
            "deny {domain} on {} ({hwid}): {reason}",
            context_path.display()
        ))?;
        Ok(false)
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
\"deny_reason\":\"{reason}\",\"allowlist_version\":\"{version}\"}}\n",
            ts = self.kernel.now_unix(),
            hwid = json_escape(hwid),
            domain = json_escape(domain),
            requested_state = requested_state,
            reason = json_escape(reason),
            version = json_escape(version),
        );
        append_log(&self.audit_path, &line)
    }

    pub(crate) fn apply(&mut self, action: &Action) -> io::Result<()> {
        // optid-safety: gate ALL dynamic Actions on boot_state.apply_armed.
        // This is the single chokepoint — there is no alternate actuator path
        // that bypasses this gate. When apply_armed is false (config failure,
        // dry-run, or competing-daemon downgrade), every Action is skipped
        // with a logged reason. The curated baseline is applied separately
        // via apply_baseline(), gated by baseline_armed.
        if !self.dynamic_writes_armed()? {
            return Ok(());
        }
        // SPEC §3 contract gate. Evaluated before capability validation,
        // the hardware allowlist, journaling and any mutation, so a
        // blocked action leaves no trace on the device or in the state
        // directory.
        if !self.contract_permits(action)? {
            return Ok(());
        }
        match action {
            Action::CpuEpp { value, .. } => {
                let paths = discover_cpu_epp_paths_with(self.kernel.as_ref());
                if paths.is_empty() {
                    self.log("skip cpu.epp: no energy_performance_preference paths")?;
                    return Ok(());
                }
                for path in paths {
                    // Phase 5 hardening: typed capability check before
                    // guarded_write. Defence-in-depth against path discovery
                    // changes.
                    if let Err(e) = Capability::CpuEpp.validate_target(&path) {
                        self.log(&format!(
                            "skip cpu.epp {}: capability validation failed: {e}",
                            path.display()
                        ))?;
                        continue;
                    }
                    let old_value = self
                        .kernel
                        .read_to_string(&path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    // Soft-fail per CPU: a hotplug or transient EBUSY on one
                    // core should not terminate the daemon.
                    match self.kernel.write(&path, value) {
                        Ok(_) => {
                            self.log(&format!(
                                "write {} = {value} (was {old_value})",
                                path.display()
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!(
                                "skip cpu.epp {}: write failed: {e}",
                                path.display()
                            ))?;
                        }
                    }
                }
            }
            Action::PlatformProfile { value, .. } => {
                let path = Path::new("/sys/firmware/acpi/platform_profile");
                if path.exists() {
                    let old_value = self
                        .kernel
                        .read_to_string(path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    // Phase 5 hardening: typed capability check.
                    if let Err(e) = Capability::PlatformProfile.validate_target(path) {
                        self.log(&format!(
                            "skip platform.profile: capability validation failed: {e}"
                        ))?;
                        return Ok(());
                    }
                    // Soft-fail: a write rejection here should not crash the
                    // daemon. Log and move on; next cycle will retry.
                    match self.kernel.write(path, value) {
                        Ok(_) => {
                            self.log(&format!(
                                "write {} = {value} (was {old_value})",
                                path.display()
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!("skip platform.profile: write failed: {e}"))?;
                        }
                    }
                } else {
                    self.log("skip platform.profile: platform_profile is unavailable")?;
                }
            }
            Action::SystemdSetProperty {
                unit, properties, ..
            } => {
                // INVARIANT: `properties` must be produced by typed code paths
                // (Action::SystemdSetProperty constructors in Decision). It is
                // splatted directly into `systemctl set-property` argv with no
                // shell quoting. If a future code path ever lets policy.toml or
                // any other untrusted source feed strings into this Vec, this
                // becomes a systemd-syntax injection vector — guard at the
                // construction site, not here.
                let status = Command::new("systemctl")
                    .arg("set-property")
                    .arg("--runtime")
                    .arg(unit)
                    .args(properties)
                    .status();
                match status {
                    Ok(status) if status.success() => {
                        self.log(&format!(
                            "systemctl set-property --runtime {unit} {}",
                            properties.join(" ")
                        ))?;
                    }
                    Ok(status) => {
                        self.log(&format!(
                            "skip systemd.set-property {unit}: systemctl exited with {status}"
                        ))?;
                    }
                    Err(err) => {
                        self.log(&format!(
                            "skip systemd.set-property {unit}: systemctl unavailable: {err}"
                        ))?;
                    }
                }
            }
            Action::VmSysctl { path, value, .. } => {
                let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                let key = format!("vm_{filename}");

                // Phase 5 hardening: typed capability check BEFORE journaling
                // or writing. Rejects paths not matching the VmSysctl shape.
                if let Err(e) = Capability::VmSysctl.validate_target(path) {
                    self.log(&format!(
                        "skip vm.sysctl {filename}: capability validation failed: {e}"
                    ))?;
                    return Ok(());
                }

                // Back up original value if not already backed up
                let orig_file = self.state_dir.join(format!("original_{key}"));
                if !orig_file.exists() {
                    if let Ok(current_val) = self.kernel.read_to_string(path) {
                        let _ = atomic_write_state_file(&orig_file, current_val.trim());
                    }
                }

                // Write intended value
                let intended_file = self.state_dir.join(format!("intended_{key}"));
                let _ = atomic_write_state_file(&intended_file, value);

                // Write new value to sysctl path
                let old_value = self
                    .kernel
                    .read_to_string(path)
                    .ok()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match self.kernel.write(path, value) {
                    Ok(_) => {
                        mark_applied(&self.state_dir, &key, value);
                        self.log(&format!(
                            "write {} = {value} (was {old_value})",
                            path.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!("skip vm.sysctl {filename}: write failed: {e}"))?;
                    }
                }
            }
            Action::CpuDmaLatency { value, reason } => {
                let should_apply = match self.last_cpu_latency {
                    Some(last_val) => last_val != *value,
                    None => true,
                };
                if should_apply {
                    let old_value = self
                        .pmqos_sink
                        .read_cpu_latency()
                        .unwrap_or_else(|_| "n/a".to_string());
                    // Soft-fail: missing /dev/cpu_dma_latency (e.g. running in
                    // a container or on a kernel without it) should not crash
                    // the daemon. Skip and log; `last_cpu_latency` is left
                    // untouched so a future success will still take effect.
                    let val_str = value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "n/a".to_string());
                    match self.pmqos_sink.write_cpu_latency(*value) {
                        Ok(_) => {
                            self.last_cpu_latency = Some(*value);
                            // optid-safety: mark applied so crash recovery
                            // knows the write landed. The PM QoS sink has no
                            // stable sysfs path; the journal key is fixed.
                            mark_applied(&self.state_dir, "cpu_dma_latency", &val_str);
                            self.log(&format!(
                                "write /dev/cpu_dma_latency = {val_str} (was {old_value}) reason: {reason}"
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!(
                                "skip /dev/cpu_dma_latency = {val_str}: write failed: {e} reason: {reason}"
                            ))?;
                        }
                    }
                }
            }
            Action::DeviceResumeLatency {
                path,
                value,
                reason,
            } => {
                // Phase 5 hardening: typed capability check BEFORE the
                // allowlist gate or any journaling.
                if let Err(e) = Capability::DeviceResumeLatency.validate_target(path) {
                    self.log(&format!(
                        "skip device_resume_latency {}: capability validation failed: {e}",
                        path.display()
                    ))?;
                    return Ok(());
                }
                // WP-N4 safety gate: per-device runtime-PM resume latency is a
                // depth-enabler knob, so it must clear the hardware allowlist
                // before any write. Default-deny when the gate is enabled and
                // the HWID is unknown; skip (no write) on denial. Disabled by
                // default, in which case this is a no-op.
                if !self.allowlist_permits("runtime_pm", hwid_from_attr_path(path), 0, path)? {
                    return Ok(());
                }
                let should_apply = match self.last_device_latencies.get(path) {
                    Some(last_val) => last_val != value,
                    None => true,
                };
                if should_apply {
                    let hash = get_path_hash(path);
                    let key = format!("dev_{hash}");

                    // Back up original value if not already backed up
                    let orig_file = self.state_dir.join(format!("original_{key}"));
                    if !orig_file.exists() {
                        if let Ok(current_val) = self.pmqos_sink.read_device_latency(path) {
                            let content = format!("{}\n{}", path.display(), current_val.trim());
                            let _ = atomic_write_state_file(&orig_file, &content);
                        }
                    }

                    // Write intended value
                    let val_str = value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "0".to_string());
                    let intended_file = self.state_dir.join(format!("intended_{key}"));
                    let _ = atomic_write_state_file(&intended_file, &val_str);

                    let old_value = self
                        .pmqos_sink
                        .read_device_latency(path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                    match self.pmqos_sink.write_device_latency(path, &val_str) {
                        Ok(_) => {
                            self.last_device_latencies.insert(path.clone(), *value);
                            mark_applied(&self.state_dir, &key, &val_str);
                            self.log(&format!(
                                "write {} = {val_str} (was {old_value}) reason: {reason}",
                                path.display()
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!(
                                "skip device latency {}: write failed: {e}",
                                path.display()
                            ))?;
                        }
                    }
                }
            }
            Action::RuntimePm {
                device_dir,
                autosuspend_delay_ms,
                reason,
            } => {
                // Phase 5 hardening: typed capability check BEFORE the
                // allowlist gate or any journaling. Validate both target
                // paths (power/control and power/autosuspend_delay_ms).
                let control_path = device_dir.join("power").join("control");
                let delay_path = device_dir.join("power").join("autosuspend_delay_ms");
                if let Err(e) = Capability::RuntimePm.validate_target(&control_path) {
                    self.log(&format!(
                        "skip runtime_pm {}: capability validation failed for control path: {e}",
                        device_dir.display()
                    ))?;
                    return Ok(());
                }
                if delay_path.exists() {
                    if let Err(e) = Capability::RuntimePm.validate_target(&delay_path) {
                        self.log(&format!(
                            "skip runtime_pm {}: capability validation failed for delay path: {e}",
                            device_dir.display()
                        ))?;
                        return Ok(());
                    }
                }
                // WP-N5 safety gate: enabling autosuspend is a depth-enabler, so
                // it must clear the N4 allowlist (domain runtime_pm). Default-deny
                // + skip when the HWID is unknown. No-op when the gate is off.
                if !self.allowlist_permits(
                    "runtime_pm",
                    hwid_from_device_dir(device_dir),
                    0,
                    device_dir,
                )? {
                    return Ok(());
                }

                // §1.6: never autosuspend a network device whose link is up — it
                // would silently drop packets. Re-checked every cycle.
                if runtime_pm::network_carrier_up(device_dir) {
                    self.log(&format!(
                        "skip runtime_pm {}: network carrier up",
                        device_dir.display()
                    ))?;
                    return Ok(());
                }

                // §1.3: warn (do not modify) when autosuspending an input device
                // whose wakeup is disabled. optid never writes power/wakeup.
                if let Some(warning) = runtime_pm::wakeup_warning(device_dir) {
                    self.log(&format!("warn runtime_pm: {warning}"))?;
                }

                // Idempotence: skip redundant re-writes within the session.
                if self.last_runtime_pm.get(device_dir) == Some(autosuspend_delay_ms) {
                    return Ok(());
                }

                let delay_str = autosuspend_delay_ms.to_string();

                // ── Phase 6: journaled transactional application ──────────
                //
                // 1. Resolve and validate every target (done above).
                // 2. Read all original values.
                // 3. Persist the complete recovery journal durably.
                // 4. Apply writes in deterministic order: delay, then control.
                // 5. If the second write fails, roll back the first.
                // 6. If rollback fails, retain the journal and report.
                // 7. Do not mark applied until every write succeeds.
                //
                // We do NOT claim atomicity across kernel sysfs files. This
                // is journaled transactional application with compensating
                // rollback: the journal makes the operation recoverable,
                // and the rollback makes the failure mode well-defined.

                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_rpm_{hash}"));
                let intended_file = self.state_dir.join(format!("intended_rpm_{hash}"));

                // Step 2: read originals (reuse existing journal if present
                // from a previous cycle that crashed before marking applied).
                let (orig_control, orig_delay) = if orig_file.exists() {
                    let content = self.kernel.read_to_string(&orig_file).unwrap_or_default();
                    let mut lines = content.lines();
                    let _dev = lines.next();
                    let c = lines.next().unwrap_or("on").to_string();
                    let d = lines.next().unwrap_or("n/a").to_string();
                    (c, d)
                } else {
                    let c = self
                        .kernel
                        .read_to_string(&control_path)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "on".to_string());
                    let d = if delay_path.exists() {
                        self.kernel
                            .read_to_string(&delay_path)
                            .ok()
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|| "n/a".to_string())
                    } else {
                        "n/a".to_string()
                    };
                    (c, d)
                };

                // Step 3: persist the recovery journal durably BEFORE any
                // mutation so a crash during apply leaves a complete record.
                let journal_content =
                    format!("{}\n{orig_control}\n{orig_delay}", device_dir.display());
                if let Err(e) = atomic_write_state_file(&orig_file, &journal_content) {
                    self.log(&format!(
                        "skip runtime_pm {}: failed to write recovery journal: {e}",
                        device_dir.display()
                    ))?;
                    return Ok(());
                }
                let _ = atomic_write_state_file(&intended_file, &format!("auto\n{delay_str}"));

                // Step 4: apply writes in deterministic order.
                // delay first (harmless while control is still "on"),
                // then control=auto (which actually enables autosuspend).
                //
                // Test hook: `fail_nth_runtime_pm_write` (#[cfg(test)] only)
                // injects a synthetic failure at write #1, #2, or #3 to
                // exercise each failure point deterministically. In production
                // builds this field does not exist and the cfg!(test) branches
                // are compiled out.
                let delay_applied = if delay_path.exists() {
                    match self.runtime_pm_write(&delay_path, &delay_str, 1) {
                        Ok(_) => true,
                        Err(e) => {
                            self.log(&format!(
                                "skip runtime_pm delay {}: write failed (no rollback needed): {e}",
                                device_dir.display()
                            ))?;
                            false
                        }
                    }
                } else {
                    true
                };

                if !delay_applied {
                    return Ok(());
                }

                // Step 5: second write. If this fails, roll back the first.
                match self.runtime_pm_write(&control_path, "auto", 2) {
                    Ok(_) => {
                        self.last_runtime_pm
                            .insert(device_dir.clone(), *autosuspend_delay_ms);
                        let rpm_key = format!("rpm_{hash}");
                        mark_applied(&self.state_dir, &rpm_key, &format!("auto\n{delay_str}"));
                        self.log(&format!(
                            "write {} control=auto autosuspend_delay_ms={delay_str} reason: {reason}",
                            device_dir.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!(
                            "runtime_pm {}: control write failed after delay write succeeded; rolling back delay: {e}",
                            device_dir.display()
                        ))?;
                        if delay_path.exists() && orig_delay != "n/a" {
                            match self.runtime_pm_write(&delay_path, &orig_delay, 3) {
                                Ok(_) => {
                                    self.log(&format!(
                                        "runtime_pm {}: rolled back delay to {orig_delay}",
                                        device_dir.display()
                                    ))?;
                                }
                                Err(re_err) => {
                                    self.log(&format!(
                                        "runtime_pm {}: ROLLBACK FAILED — delay left at {delay_str}, control unchanged. Journal retained for recovery. Rollback error: {re_err}",
                                        device_dir.display()
                                    ))?;
                                }
                            }
                        }
                        // Do NOT mark applied. last_runtime_pm is not updated.
                    }
                }
            }
            Action::PcieAspm {
                device_dir,
                enable,
                reason,
            } => {
                // Phase 5 hardening: typed capability check.
                let aspm_path = device_dir.join("link").join("l1_aspm");
                if let Err(e) = Capability::PcieAspm.validate_target(&aspm_path) {
                    self.log(&format!(
                        "skip pcie_aspm {}: capability validation failed: {e}",
                        device_dir.display()
                    ))?;
                    return Ok(());
                }
                // WP-N6 PCIe ASPM, gated on the N4 allowlist (domain pci_aspm).
                if !self.allowlist_permits(
                    "pci_aspm",
                    hwid_from_device_dir(device_dir),
                    0,
                    device_dir,
                )? {
                    return Ok(());
                }
                // §1.4: CNVi radios are not standard PCIe endpoints; their link
                // PM is firmware-managed and l1_aspm writes do not apply. Skip.
                if storage::is_cnvi(device_dir) {
                    self.log(&format!(
                        "skip pcie_aspm {}: CNVi device (link PM is firmware-managed)",
                        device_dir.display()
                    ))?;
                    return Ok(());
                }
                if self.last_pcie_aspm.get(device_dir) == Some(enable) {
                    return Ok(());
                }

                let aspm_path = device_dir.join("link").join("l1_aspm");
                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_aspm_{hash}"));
                if !orig_file.exists() {
                    let orig = self
                        .kernel
                        .read_to_string(&aspm_path)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "0".to_string());
                    let _ = atomic_write_state_file(
                        &orig_file,
                        &format!("{}\n{orig}", device_dir.display()),
                    );
                }
                let val = if *enable { "1" } else { "0" };
                let intended_file = self.state_dir.join(format!("intended_aspm_{hash}"));
                let _ = atomic_write_state_file(&intended_file, val);

                match self.kernel.write(&aspm_path, val) {
                    Ok(_) => {
                        self.last_pcie_aspm.insert(device_dir.clone(), *enable);
                        let aspm_key = format!("aspm_{hash}");
                        mark_applied(&self.state_dir, &aspm_key, val);
                        self.log(&format!(
                            "write {} l1_aspm={val} reason: {reason}",
                            aspm_path.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!(
                            "skip pcie_aspm {}: write failed: {e}",
                            device_dir.display()
                        ))?;
                    }
                }
            }
            Action::SataAlpm {
                host_dir,
                policy,
                reason,
            } => {
                // Phase 5 hardening: typed capability check.
                let policy_path = host_dir.join("link_power_management_policy");
                if let Err(e) = Capability::SataAlpm.validate_target(&policy_path) {
                    self.log(&format!(
                        "skip sata_alpm {}: capability validation failed: {e}",
                        host_dir.display()
                    ))?;
                    return Ok(());
                }
                // WP-N6 SATA ALPM, gated on the N4 allowlist (domain sata_alpm).
                // The scsi_host has no modalias of its own — resolve the backing
                // PCI controller's HWID by walking ancestors.
                if !self.allowlist_permits(
                    "sata_alpm",
                    hwid_from_ancestors(host_dir),
                    0,
                    host_dir,
                )? {
                    return Ok(());
                }
                if self.last_sata_alpm.get(host_dir).map(String::as_str) == Some(policy.as_str()) {
                    return Ok(());
                }

                let policy_path = host_dir.join("link_power_management_policy");
                let hash = get_path_hash(host_dir);
                let orig_file = self.state_dir.join(format!("original_alpm_{hash}"));
                if !orig_file.exists() {
                    let orig = self
                        .kernel
                        .read_to_string(&policy_path)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "max_performance".to_string());
                    let _ = atomic_write_state_file(
                        &orig_file,
                        &format!("{}\n{orig}", host_dir.display()),
                    );
                }
                let intended_file = self.state_dir.join(format!("intended_alpm_{hash}"));
                let _ = atomic_write_state_file(&intended_file, policy);

                match self.kernel.write(&policy_path, policy) {
                    Ok(_) => {
                        self.last_sata_alpm.insert(host_dir.clone(), policy.clone());
                        let alpm_key = format!("alpm_{hash}");
                        mark_applied(&self.state_dir, &alpm_key, policy);
                        self.log(&format!(
                            "write {} policy={policy} reason: {reason}",
                            policy_path.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!(
                            "skip sata_alpm {}: write failed: {e}",
                            host_dir.display()
                        ))?;
                    }
                }
            }
            Action::Backlight {
                device_dir,
                target_pct,
                reason,
            } => {
                // Phase 5 hardening: typed capability check.
                let bright_path = device_dir.join("brightness");
                if let Err(e) = Capability::Backlight.validate_target(&bright_path) {
                    self.log(&format!(
                        "skip backlight {}: capability validation failed: {e}",
                        device_dir.display()
                    ))?;
                    return Ok(());
                }
                // WP-N7 backlight, gated on the N4 allowlist (domain backlight,
                // HWID from the backing GPU via ancestor-walk).
                if !self.allowlist_permits(
                    "backlight",
                    hwid_from_ancestors(device_dir),
                    0,
                    device_dir,
                )? {
                    return Ok(());
                }
                let max = match display::read_max_brightness(device_dir) {
                    Some(m) if m > 0 => m,
                    _ => {
                        self.log(&format!(
                            "skip backlight {}: no usable max_brightness",
                            device_dir.display()
                        ))?;
                        return Ok(());
                    }
                };
                // Floor-clamped target — never black, never below the interactive floor.
                let target = display::compute_target_brightness(max, *target_pct);
                if self.last_backlight.get(device_dir) == Some(&target) {
                    return Ok(());
                }

                let bright_path = device_dir.join("brightness");
                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_bl_{hash}"));
                if !orig_file.exists() {
                    let orig = self
                        .kernel
                        .read_to_string(&bright_path)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    let _ = atomic_write_state_file(
                        &orig_file,
                        &format!("{}\n{orig}", device_dir.display()),
                    );
                }
                let target_str = target.to_string();
                let intended_file = self.state_dir.join(format!("intended_bl_{hash}"));
                let _ = atomic_write_state_file(&intended_file, &target_str);

                match self.kernel.write(&bright_path, &target_str) {
                    Ok(_) => {
                        self.last_backlight.insert(device_dir.clone(), target);
                        let bl_key = format!("bl_{hash}");
                        mark_applied(&self.state_dir, &bl_key, &target_str);
                        self.log(&format!(
                            "write {} brightness={target_str} (target {target_pct}% of {max}) reason: {reason}",
                            bright_path.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!(
                            "skip backlight {}: write failed: {e}",
                            device_dir.display()
                        ))?;
                    }
                }
            }
        }
        Ok(())
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
    pub(crate) fn revert_key(&mut self, key: &str) -> io::Result<bool> {
        let orig_file = self.state_dir.join(format!("original_{key}"));
        if !orig_file.exists() {
            return Ok(false);
        }
        let Ok(content) = fs::read_to_string(&orig_file) else {
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
            clear_journal(&self.state_dir, key);
        } else {
            self.log(&format!(
                "context-change revert {key}: journal retained; restore did not complete"
            ))?;
        }
        Ok(restored)
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        append_log(
            &self.log_path,
            &format!("{} {message}\n", self.kernel.now_unix()),
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
