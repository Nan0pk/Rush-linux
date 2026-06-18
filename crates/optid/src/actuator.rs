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
    pub(crate) pmqos_sink: Box<dyn PmqosSink>,
    pub(crate) last_cpu_latency: Option<Option<i32>>,
    pub(crate) last_device_latencies: HashMap<PathBuf, Option<i32>>,
}

impl Actuator {
    pub(crate) fn new(state_dir: PathBuf) -> Self {
        let log_path = state_dir.join("actions.log");
        Self {
            state_dir,
            log_path,
            pmqos_sink: Box::new(RealPmqosSink::new()),
            last_cpu_latency: None,
            last_device_latencies: HashMap::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn new_with_sink(state_dir: PathBuf, sink: Box<dyn PmqosSink>) -> Self {
        let log_path = state_dir.join("actions.log");
        Self {
            state_dir,
            log_path,
            pmqos_sink: sink,
            last_cpu_latency: None,
            last_device_latencies: HashMap::new(),
        }
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
        }
        Ok(())
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        append_log(&self.log_path, &format!("{} {message}\n", now_unix()))
    }
}
