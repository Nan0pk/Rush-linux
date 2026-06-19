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
use crate::actuators::runtime_pm;
use crate::allowlist::{hwid_from_attr_path, hwid_from_device_dir, Allowlist, Verdict};
use crate::io_util::{append_log, atomic_write_state_file, get_path_hash, guarded_write};
use crate::sensors::{discover_cpu_epp_paths, now_unix};

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
        guarded_write(device_path, value)
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
    /// WP-N4 hardware allowlist gate. `None` ⇒ gate disabled (the v0.x default):
    /// the actuator behaves exactly as before. `Some(_)` ⇒ depth-enabler writes
    /// are default-denied unless the device HWID is allowlisted, and every
    /// denial is appended to `audit_path` with its reason.
    pub(crate) allowlist: Option<Allowlist>,
}

impl Actuator {
    pub(crate) fn new(state_dir: PathBuf) -> Self {
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
            allowlist: None,
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
            allowlist: None,
        }
    }

    /// Enable the WP-N4 hardware allowlist gate with the given (already loaded)
    /// allowlist. Called from `main` when `--allowlist` is set.
    pub(crate) fn enable_allowlist(&mut self, allowlist: Allowlist) {
        self.allowlist = Some(allowlist);
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
            ts = now_unix(),
            hwid = json_escape(hwid),
            domain = json_escape(domain),
            requested_state = requested_state,
            reason = json_escape(reason),
            version = json_escape(version),
        );
        append_log(&self.audit_path, &line)
    }

    pub(crate) fn apply(&mut self, action: &Action) -> io::Result<()> {
        match action {
            Action::CpuEpp { value, .. } => {
                let paths = discover_cpu_epp_paths();
                if paths.is_empty() {
                    self.log("skip cpu.epp: no energy_performance_preference paths")?;
                    return Ok(());
                }
                for path in paths {
                    let old_value = fs::read_to_string(&path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    // Soft-fail per CPU: a hotplug or transient EBUSY on one
                    // core should not terminate the daemon.
                    match guarded_write(&path, value) {
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
                    let old_value = fs::read_to_string(path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    // Soft-fail: a write rejection here should not crash the
                    // daemon. Log and move on; next cycle will retry.
                    match guarded_write(path, value) {
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

                // Back up original value if not already backed up
                let orig_file = self.state_dir.join(format!("original_{key}"));
                if !orig_file.exists() {
                    if let Ok(current_val) = fs::read_to_string(path) {
                        let _ = atomic_write_state_file(&orig_file, current_val.trim());
                    }
                }

                // Write intended value
                let intended_file = self.state_dir.join(format!("intended_{key}"));
                let _ = atomic_write_state_file(&intended_file, value);

                // Write new value to sysctl path
                let old_value = fs::read_to_string(path)
                    .ok()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match guarded_write(path, value) {
                    Ok(_) => {
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

                let control_path = device_dir.join("power").join("control");
                let delay_path = device_dir.join("power").join("autosuspend_delay_ms");
                let delay_str = autosuspend_delay_ms.to_string();

                // Journal originals once (device dir + control + delay) so
                // revert_runtime_pm can restore them on stop.
                let hash = get_path_hash(device_dir);
                let orig_file = self.state_dir.join(format!("original_rpm_{hash}"));
                if !orig_file.exists() {
                    let orig_control = fs::read_to_string(&control_path)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "on".to_string());
                    let orig_delay = fs::read_to_string(&delay_path)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "n/a".to_string());
                    let content = format!("{}\n{orig_control}\n{orig_delay}", device_dir.display());
                    let _ = atomic_write_state_file(&orig_file, &content);
                }
                let intended_file = self.state_dir.join(format!("intended_rpm_{hash}"));
                let _ = atomic_write_state_file(&intended_file, &format!("auto\n{delay_str}"));

                // Set the delay first (harmless while control is still "on"),
                // then enable autosuspend. Soft-fail each write independently.
                if delay_path.exists() {
                    if let Err(e) = guarded_write(&delay_path, &delay_str) {
                        self.log(&format!(
                            "skip runtime_pm delay {}: write failed: {e}",
                            device_dir.display()
                        ))?;
                    }
                }
                match guarded_write(&control_path, "auto") {
                    Ok(_) => {
                        self.last_runtime_pm
                            .insert(device_dir.clone(), *autosuspend_delay_ms);
                        self.log(&format!(
                            "write {} control=auto autosuspend_delay_ms={delay_str} reason: {reason}",
                            device_dir.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!(
                            "skip runtime_pm {}: control write failed: {e}",
                            device_dir.display()
                        ))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        append_log(&self.log_path, &format!("{} {message}\n", now_unix()))
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
