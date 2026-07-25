//! The `Action` enum — the closed vocabulary of policy mutations `optid` may
//! emit. Each variant carries a `reason` string that `optctl explain` will
//! surface, so every action is self-documenting.
//!
//! Constructors are intentionally typed (`cpu_epp`, `platform_profile`, …)
//! rather than direct struct literals, so future tightening (allowlists,
//! value validation) can land at the construction site without touching the
//! actuator.

use std::path::PathBuf;

use crate::io_util::get_path_hash;
use crate::policy::Domain;

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
    /// WP-N6 PCIe ASPM. Toggles a PCI device's `link/l1_aspm` (L1 substates).
    /// Allowlist-gated (domain `pci_aspm`) and reverted on stop. `device_dir`
    /// is the PCI device sysfs directory; `enable` writes `1`/`0`.
    PcieAspm {
        device_dir: PathBuf,
        enable: bool,
        reason: String,
    },
    /// WP-N6 SATA ALPM. Sets a SCSI host's `link_power_management_policy`.
    /// Allowlist-gated (domain `sata_alpm`, HWID resolved from the host's
    /// backing PCI controller) and reverted on stop. `host_dir` is the
    /// `/sys/class/scsi_host/hostN` directory.
    SataAlpm {
        host_dir: PathBuf,
        policy: String,
        reason: String,
    },
    /// WP-N7 display depth: reduce panel backlight to `target_pct` of
    /// `max_brightness` on battery-idle. Allowlist-gated (domain `backlight`,
    /// HWID resolved from the backing GPU), floor-clamped so the screen never
    /// goes black, journaled, and reverted on stop. `device_dir` is a
    /// `/sys/class/backlight/<name>` directory.
    Backlight {
        device_dir: PathBuf,
        target_pct: u8,
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

    /// F1 — The actuation domain this `Action` belongs to. Every variant
    /// returns its domain so the per-domain `DomainMode` gate applies
    /// uniformly. Prior to the F1 package-completion repair, the
    /// `SystemdSetProperty` variant returned `None` (a fail-open hole:
    /// cgroup reweighting was not subject to the per-domain gate). It now
    /// returns `Some(Domain::CgroupReweight)`, and `decide_resolved` will
    /// filter it like every other variant when the operator sets
    /// `[domains.cgroup_reweight] mode = "off"` or `"observe"`.
    ///
    /// Keep this in lockstep with `crate::capability::Capability::allowlist_domain`
    /// and `crate::policy::Domain::as_str`. The `action_domain_matches_capability`
    /// test enforces the triple-stay-in-sync invariant.
    pub(crate) fn domain(&self) -> Option<Domain> {
        match self {
            Action::CpuEpp { .. } => Some(Domain::CpuEpp),
            Action::PlatformProfile { .. } => Some(Domain::PlatformProfile),
            Action::SystemdSetProperty { .. } => Some(Domain::CgroupReweight),
            Action::VmSysctl { .. } => Some(Domain::VmSysctl),
            Action::CpuDmaLatency { .. } => Some(Domain::CpuDmaLatency),
            Action::DeviceResumeLatency { .. } => Some(Domain::DeviceResumeLatency),
            Action::RuntimePm { .. } => Some(Domain::RuntimePm),
            Action::PcieAspm { .. } => Some(Domain::PcieAspm),
            Action::SataAlpm { .. } => Some(Domain::SataAlpm),
            Action::Backlight { .. } => Some(Domain::Backlight),
        }
    }

    /// The revert-journal key this action writes under, matching the
    /// `original_<key>` / `intended_<key>` / `applied_<key>` state files
    /// created by `Actuator::apply`.
    ///
    /// Keep this in lockstep with the journal keys built inside
    /// `Actuator::apply`; `journal_key_matches_actuator_keys` in
    /// `actuator.rs` pins the per-device hash forms against
    /// `get_path_hash`, and the context-change revert in `main` relies on
    /// these keys being exactly the ones the actuator journaled.
    ///
    /// Returns `None` for `SystemdSetProperty`: it shells out to
    /// `systemctl set-property --runtime`, writes no sysfs file and
    /// journals nothing, so there is no key to revert. Its `--runtime`
    /// properties are already dropped by systemd on reboot.
    pub(crate) fn journal_key(&self) -> Option<String> {
        match self {
            Action::CpuEpp { .. } => Some("cpu_epp".to_string()),
            Action::PlatformProfile { .. } => Some("platform_profile".to_string()),
            Action::SystemdSetProperty { .. } => None,
            // vm sysctls journal per-knob, keyed by the sysctl file name
            // (e.g. `vm_swappiness`), matching the `vm_{filename}` key
            // the actuator builds.
            Action::VmSysctl { path, .. } => {
                let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                Some(format!("vm_{filename}"))
            }
            Action::CpuDmaLatency { .. } => Some("cpu_dma_latency".to_string()),
            // Per-device keys are hashed from the same path the actuator
            // hashes: the attribute path for PM QoS, the device/host
            // directory for the rest.
            Action::DeviceResumeLatency { path, .. } => {
                Some(format!("dev_{}", get_path_hash(path)))
            }
            Action::RuntimePm { device_dir, .. } => {
                Some(format!("rpm_{}", get_path_hash(device_dir)))
            }
            Action::PcieAspm { device_dir, .. } => {
                Some(format!("aspm_{}", get_path_hash(device_dir)))
            }
            Action::SataAlpm { host_dir, .. } => Some(format!("alpm_{}", get_path_hash(host_dir))),
            Action::Backlight { device_dir, .. } => {
                Some(format!("bl_{}", get_path_hash(device_dir)))
            }
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
            Self::PcieAspm {
                device_dir,
                enable,
                reason,
            } => {
                // Prefix matches Domain::PcieAspm.as_str() ("pci_aspm") so
                // Decision::render would_act= lines and domain keys stay aligned.
                format!(
                    "pci_aspm {} l1_aspm={} ({reason})",
                    device_dir.display(),
                    if *enable { 1 } else { 0 }
                )
            }
            Self::SataAlpm {
                host_dir,
                policy,
                reason,
            } => {
                format!(
                    "sata_alpm {} policy={policy} ({reason})",
                    host_dir.display()
                )
            }
            Self::Backlight {
                device_dir,
                target_pct,
                reason,
            } => {
                format!(
                    "backlight {} target={target_pct}% ({reason})",
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
    fn journal_key_matches_actuator_key_shapes() {
        // System-wide knobs use fixed keys.
        assert_eq!(
            Action::CpuEpp {
                value: "power".to_string(),
                reason: "t".to_string(),
            }
            .journal_key()
            .as_deref(),
            Some("cpu_epp")
        );
        assert_eq!(
            Action::PlatformProfile {
                value: "low-power".to_string(),
                reason: "t".to_string(),
            }
            .journal_key()
            .as_deref(),
            Some("platform_profile")
        );
        assert_eq!(
            Action::CpuDmaLatency {
                value: Some(100),
                reason: "t".to_string(),
            }
            .journal_key()
            .as_deref(),
            Some("cpu_dma_latency")
        );

        // vm sysctls are keyed per knob, matching `vm_{filename}`.
        assert_eq!(
            Action::vm_sysctl(
                PathBuf::from("/proc/sys/vm/swappiness"),
                "100".to_string(),
                "t".to_string(),
            )
            .journal_key()
            .as_deref(),
            Some("vm_swappiness")
        );

        // systemctl set-property journals nothing.
        assert!(Action::systemd_set_property(
            "user.slice".to_string(),
            vec!["CPUWeight=100".to_string()],
            "t".to_string(),
        )
        .journal_key()
        .is_none());

        // Per-device keys hash the same path the actuator hashes.
        let attr =
            PathBuf::from("/sys/devices/pci0000:00/0000:00:14.0/power/pm_qos_resume_latency_us");
        assert_eq!(
            Action::DeviceResumeLatency {
                path: attr.clone(),
                value: Some(0),
                reason: "t".to_string(),
            }
            .journal_key(),
            Some(format!("dev_{}", get_path_hash(&attr)))
        );

        let dev = PathBuf::from("/sys/bus/usb/devices/1-1");
        assert_eq!(
            Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: 2000,
                reason: "t".to_string(),
            }
            .journal_key(),
            Some(format!("rpm_{}", get_path_hash(&dev)))
        );
        assert_eq!(
            Action::PcieAspm {
                device_dir: dev.clone(),
                enable: true,
                reason: "t".to_string(),
            }
            .journal_key(),
            Some(format!("aspm_{}", get_path_hash(&dev)))
        );
        assert_eq!(
            Action::SataAlpm {
                host_dir: dev.clone(),
                policy: "med_power_with_dipm".to_string(),
                reason: "t".to_string(),
            }
            .journal_key(),
            Some(format!("alpm_{}", get_path_hash(&dev)))
        );
        assert_eq!(
            Action::Backlight {
                device_dir: dev.clone(),
                target_pct: 40,
                reason: "t".to_string(),
            }
            .journal_key(),
            Some(format!("bl_{}", get_path_hash(&dev)))
        );
    }

    #[test]
    fn every_action_variant_has_a_defined_journal_key_decision() {
        // One entry per Action variant: either a key or an explicit None.
        // A new variant added without a `journal_key` arm is a compile
        // error in `journal_key` itself; this test pins the *intent* so a
        // future variant cannot silently default to `None` and skip the
        // context-change revert.
        let variants: Vec<(Action, bool)> = vec![
            (
                Action::CpuEpp {
                    value: "power".to_string(),
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::PlatformProfile {
                    value: "low-power".to_string(),
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::systemd_set_property("user.slice".to_string(), vec![], "t".to_string()),
                false,
            ),
            (
                Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/swappiness"),
                    "100".to_string(),
                    "t".to_string(),
                ),
                true,
            ),
            (
                Action::CpuDmaLatency {
                    value: None,
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::DeviceResumeLatency {
                    path: PathBuf::from("/sys/x/power/pm_qos_resume_latency_us"),
                    value: None,
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::RuntimePm {
                    device_dir: PathBuf::from("/sys/x"),
                    autosuspend_delay_ms: 1,
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::PcieAspm {
                    device_dir: PathBuf::from("/sys/x"),
                    enable: false,
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::SataAlpm {
                    host_dir: PathBuf::from("/sys/x"),
                    policy: "max_performance".to_string(),
                    reason: "t".to_string(),
                },
                true,
            ),
            (
                Action::Backlight {
                    device_dir: PathBuf::from("/sys/x"),
                    target_pct: 50,
                    reason: "t".to_string(),
                },
                true,
            ),
        ];
        assert_eq!(
            variants.len(),
            10,
            "add the new Action variant to this table and to journal_key()"
        );
        for (action, expect_key) in variants {
            assert_eq!(
                action.journal_key().is_some(),
                expect_key,
                "unexpected journal_key presence for {}",
                action.describe()
            );
        }
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
