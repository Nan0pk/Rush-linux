//! The `Action` enum — the closed vocabulary of policy mutations `optid` may
//! emit. Each variant carries a `reason` string that `optctl explain` will
//! surface, so every action is self-documenting.
//!
//! Constructors are intentionally typed (`cpu_epp`, `platform_profile`, …)
//! rather than direct struct literals, so future tightening (allowlists,
//! value validation) can land at the construction site without touching the
//! actuator.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) enum Action {
    CpuEpp {
        value: String,
        reason: String,
    },
    PlatformProfile {
        value: String,
        reason: String,
    },
    SystemdSetProperty {
        unit: String,
        properties: Vec<String>,
        reason: String,
    },
    VmSysctl {
        path: PathBuf,
        value: String,
        reason: String,
    },
    CpuDmaLatency {
        value: Option<i32>,
        reason: String,
    },
    DeviceResumeLatency {
        path: PathBuf,
        value: Option<i32>,
        reason: String,
    },
    /// WP-N5 runtime-PM autosuspend. Enables `power/control=auto` and sets
    /// `power/autosuspend_delay_ms` on a device directory. Allowlist-gated
    /// (domain `runtime_pm`) and reverted on stop. `device_dir` is the device's
    /// sysfs directory (e.g. `/sys/bus/usb/devices/1-1`), not an attribute file.
    RuntimePm {
        device_dir: PathBuf,
        autosuspend_delay_ms: i32,
        reason: String,
    },
}

impl Action {
    pub(crate) fn cpu_epp(value: String, reason: String) -> Self {
        Self::CpuEpp { value, reason }
    }

    pub(crate) fn platform_profile(value: String, reason: String) -> Self {
        Self::PlatformProfile { value, reason }
    }

    pub(crate) fn systemd_set_property(
        unit: String,
        properties: Vec<String>,
        reason: String,
    ) -> Self {
        Self::SystemdSetProperty {
            unit,
            properties,
            reason,
        }
    }

    pub(crate) fn vm_sysctl(path: PathBuf, value: String, reason: String) -> Self {
        Self::VmSysctl {
            path,
            value,
            reason,
        }
    }

    /// One-line human-readable description, used by `optctl explain` and the
    /// `decisions.log` audit trail.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::CpuEpp { value, reason } => format!("cpu.epp={value} ({reason})"),
            Self::PlatformProfile { value, reason } => {
                format!("platform.profile={value} ({reason})")
            }
            Self::SystemdSetProperty {
                unit,
                properties,
                reason,
            } => format!(
                "systemd.set-property {unit} {} ({reason})",
                properties.join(" ")
            ),
            Self::VmSysctl {
                path,
                value,
                reason,
            } => {
                format!("vm.sysctl {}={value} ({reason})", path.display())
            }
            Self::CpuDmaLatency { value, reason } => {
                let val_str = value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "None".to_string());
                format!("cpu_dma_latency={val_str} ({reason})")
            }
            Self::DeviceResumeLatency {
                path,
                value,
                reason,
            } => {
                let val_str = value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "None".to_string());
                format!(
                    "device_resume_latency {}={val_str} ({reason})",
                    path.display()
                )
            }
            Self::RuntimePm {
                device_dir,
                autosuspend_delay_ms,
                reason,
            } => {
                format!(
                    "runtime_pm {} control=auto autosuspend_delay_ms={autosuspend_delay_ms} ({reason})",
                    device_dir.display()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t5_explainability() {
        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "100".to_string(),
            "adjust swappiness for current mode".to_string(),
        );
        let desc = action.describe();
        assert_eq!(
            desc,
            "vm.sysctl /proc/sys/vm/swappiness=100 (adjust swappiness for current mode)"
        );
    }

    #[test]
    fn describe_handles_none_latency_values() {
        let a = Action::CpuDmaLatency {
            value: None,
            reason: "test".to_string(),
        };
        assert_eq!(a.describe(), "cpu_dma_latency=None (test)");

        let b = Action::DeviceResumeLatency {
            path: PathBuf::from("/sys/bus/pci/devices/0000:00:00.0/power/pm_qos_resume_latency_us"),
            value: None,
            reason: "test".to_string(),
        };
        assert!(b.describe().contains("=None"));
    }
}
