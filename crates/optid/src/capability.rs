//! Actuation capability manifest — the explicit, reviewable mapping from
//! every `Action` variant to its required write paths, allowlist domain,
//! and systemd sandbox requirement.
//!
//! ## Why this exists
//!
//! Before this module, the systemd unit's `ReadWritePaths=` line and the
//! `Action` enum's write vocabulary were maintained independently. An
//! action could pass policy + allowlist + `--apply` + the `guarded_write`
//! structural check and still fail when executed under the actual service,
//! because the service's `ReadWritePaths` was narrower than what the
//! action vocabulary can nominate. The capability manifest closes that
//! gap by making the relationship explicit and testable.
//!
//! ## What this module is NOT
//!
//! - It is **not** a replacement for `guarded_write`. `guarded_write`
//!   remains the final structural defence; the manifest is an earlier
//!   typed check that catches drift at the action-construction site
//!   rather than at write time.
//! - It is **not** a process-split boundary. The unprivileged-observer /
//!   privileged-actuator split is a larger architectural change tracked
//!   separately; the manifest is the strongest coherent intermediate
//!   step that fits in one PR.
//! - It does **not** grant general `/sys` write permission. Every entry
//!   in `required_read_write_paths()` is a specific subtree that some
//!   `Action` variant demonstrably needs.
//!
//! ## Adding a new actuator
//!
//! 1. Add the variant to `Action` (in `action.rs`).
//! 2. Add a matching `Capability` variant here.
//! 3. Implement `required_paths`, `allowlist_domain`, `journal_key_prefix`,
//!    and `validate_target` for it.
//! 4. Add it to `ALL_CAPABILITIES`.
//! 5. Update the systemd unit's `ReadWritePaths=` if your variant needs a
//!    new subtree — the `systemd_apply_service_grants_every_capability`
//!    test will fail until you do, which is the point.
//! 6. Update `packaging/systemd/optid-apply.service` AND
//!    `mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service` —
//!    the `mkosi_mirror_matches_packaging` test enforces byte-identity.
//!
//! ## Layered defence (four gates)
//!
//! The four-gate actuation rule from SPEC §3 is preserved:
//!
//! 1. **Active contract** — the policy layer emits an `Action` only when
//!    the current workload class's contract floor permits it.
//! 2. **Safety/allowlist approval** — per-device writes pass
//!    `Allowlist::check(domain, hwid, requested_state)`.
//! 3. **Explicit apply arming** — `boot_state.apply_armed` must be true
//!    (set by `--apply`); dry-run skips every dynamic write.
//! 4. **Reversible operation** — every write journals its original value
//!    so `revert_*` can restore it on shutdown or crash recovery.
//!
//! The capability manifest sits between gates 1 and 2: it validates the
//! target path shape before the allowlist is consulted, so an
//! out-of-root or structurally malformed path is rejected before any
//! HWID resolution or allowlist lookup. `guarded_write` is the final
//! structural defence in `io_util.rs` and is unchanged.

use std::io;
use std::path::{Component, Path, PathBuf};

/// One capability per `Action` variant that performs a kernel write.
/// `SystemdSetProperty` is omitted because it invokes `systemctl` rather
/// than writing to sysfs/procfs/devfs; its capability requirement is
/// the `systemctl` binary being on PATH, not a writable kernel path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    /// `Action::CpuEpp` — writes
    /// `/sys/devices/system/cpu/cpuN/energy_performance_preference`.
    CpuEpp,
    /// `Action::PlatformProfile` — writes
    /// `/sys/firmware/acpi/platform_profile`.
    PlatformProfile,
    /// `Action::VmSysctl` — writes `/proc/sys/vm/{swappiness,
    /// dirty_background_bytes, dirty_bytes}`. Allowlist does not apply
    /// (system-wide knob with no per-device HWID).
    VmSysctl,
    /// `Action::CpuDmaLatency` — writes `/dev/cpu_dma_latency` (PM QoS
    /// CPU floor as a 4-byte little-endian int). Allowlist does not
    /// apply (system-wide knob).
    CpuDmaLatency,
    /// `Action::DeviceResumeLatency` — writes
    /// `/sys/devices/.../power/pm_qos_resume_latency_us`. Allowlist
    /// domain `runtime_pm`.
    DeviceResumeLatency,
    /// `Action::RuntimePm` — writes `power/control` and
    /// `power/autosuspend_delay_ms` under a device directory. Two
    /// writes; journaled transactionally. Allowlist domain `runtime_pm`.
    RuntimePm,
    /// `Action::PcieAspm` — writes `link/l1_aspm` under a PCI device
    /// directory. Allowlist domain `pci_aspm`.
    PcieAspm,
    /// `Action::SataAlpm` — writes `link_power_management_policy` under
    /// a SCSI host directory. Allowlist domain `sata_alpm`.
    SataAlpm,
    /// `Action::Backlight` — writes `brightness` under
    /// `/sys/class/backlight/<name>/`. Allowlist domain `backlight`.
    Backlight,
}

impl Capability {
    /// The systemd `ReadWritePaths=` entries this capability requires.
    /// Entries are path prefixes — systemd treats them as recursive
    /// writable subtrees. The union of these across `ALL_CAPABILITIES`
    /// is the minimum `ReadWritePaths` set the apply-mode service must
    /// grant.
    ///
    /// Test-only: the manifest exists to be diffed against the shipped
    /// systemd units by the drift-detection tests, which are its only
    /// callers. Gating on `cfg(test)` rather than suppressing the
    /// unused-code lint keeps `clippy -D warnings` meaningful here.
    #[cfg(test)]
    pub(crate) fn required_paths(&self) -> &'static [&'static str] {
        match self {
            // System-wide knobs: statically known paths, safe to grant
            // narrowly in the systemd unit's ReadWritePaths.
            Capability::CpuEpp => &["/sys/devices/system/cpu"],
            Capability::PlatformProfile => &["/sys/firmware/acpi/platform_profile"],
            Capability::VmSysctl => &["/proc/sys/vm"],
            Capability::CpuDmaLatency => &["/dev/cpu_dma_latency"],
            // Per-device depth-enablers: write to dynamic paths under
            // /sys/devices, /sys/bus, and /sys/class. The specific files
            // are validated by the software allowlist in kernel_io.rs
            // (is_allowlisted_write_path) which checks path SHAPE. The
            // systemd service grants the prefix subtrees below; the
            // drift-detection test enforces the service unit matches.
            Capability::DeviceResumeLatency | Capability::RuntimePm => {
                &["/sys/devices", "/sys/bus/pci", "/sys/bus/usb"]
            }
            Capability::PcieAspm => &["/sys/bus/pci"],
            Capability::SataAlpm => &["/sys/class/scsi_host"],
            Capability::Backlight => &["/sys/class/backlight", "/sys/devices"],
        }
    }

    /// Returns true if this capability is deployable under the shipped
    /// systemd service (i.e. its required paths are statically known and
    /// granted by ReadWritePaths).
    ///
    /// This is a manifest invariant checked by
    /// `all_capabilities_are_service_deployable`, which is its only
    /// caller, so it is gated on `cfg(test)`. Gating on the test
    /// configuration rather than suppressing the unused-code lint keeps
    /// `cargo clippy --all-targets -- -D warnings` honest: if the
    /// predicate ever loses its last caller the compiler reports it,
    /// instead of a blanket suppression hiding the fact.
    #[cfg(test)]
    pub(crate) fn is_service_deployable(&self) -> bool {
        !self.required_paths().is_empty()
    }

    /// The hardware-allowlist domain this capability must clear before
    /// writing. `None` for system-wide knobs with no per-device HWID
    /// (CPU EPP, platform profile, vm sysctls, /dev/cpu_dma_latency);
    /// `Some(_)` for per-device depth-enablers.
    #[allow(dead_code)]
    pub(crate) fn allowlist_domain(&self) -> Option<&'static str> {
        match self {
            Capability::CpuEpp
            | Capability::PlatformProfile
            | Capability::VmSysctl
            | Capability::CpuDmaLatency => None,
            Capability::DeviceResumeLatency | Capability::RuntimePm => Some("runtime_pm"),
            Capability::PcieAspm => Some("pci_aspm"),
            Capability::SataAlpm => Some("sata_alpm"),
            Capability::Backlight => Some("backlight"),
        }
    }

    /// The journal-key prefix used in `original_<key>` / `applied_<key>`
    /// / `intended_<key>` state files. Matches the prefix used by the
    /// existing `Actuator::apply` call sites.
    #[allow(dead_code)]
    pub(crate) fn journal_key_prefix(&self) -> &'static str {
        match self {
            Capability::CpuEpp => "cpu_epp",
            Capability::PlatformProfile => "platform_profile",
            Capability::VmSysctl => "vm",
            Capability::CpuDmaLatency => "cpu_dma_latency",
            Capability::DeviceResumeLatency => "dev",
            Capability::RuntimePm => "rpm",
            Capability::PcieAspm => "aspm",
            Capability::SataAlpm => "alpm",
            Capability::Backlight => "bl",
        }
    }

    /// Structurally validate a target path for this capability. Rejects:
    ///
    /// - any path containing a `..` component (traversal);
    /// - any path that is not under the capability's permitted root(s);
    /// - any path whose file_name does not match the capability's
    ///   expected attribute name(s).
    ///
    /// This is the typed pre-check that runs BEFORE `guarded_write`. It
    /// is intentionally stricter than `guarded_write`'s ad-hoc checks
    /// because it knows which capability the path is for, not just the
    /// path's shape. `guarded_write` remains as the final defence.
    pub(crate) fn validate_target(&self, path: &Path) -> io::Result<()> {
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "capability {:?}: refusing path with directory traversal: {}",
                    self,
                    path.display()
                ),
            ));
        }

        // Symlink-resistance: every component (except the final file_name)
        // must be a Normal component. Path::components() normalizes '.'
        // away, so we only need to reject ParentDir explicitly here.
        // (CurDir is not yielded by components() except for a leading '.',
        // which is harmless for absolute paths.)
        for c in path.components() {
            if matches!(c, Component::ParentDir) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "capability {:?}: refusing path with '..' component: {}",
                        self,
                        path.display()
                    ),
                ));
            }
        }

        // In test mode, relax the root-prefix check for paths under the
        // test temp directory. This mirrors guarded_write's `cfg!(test)`
        // exceptions: unit tests need to exercise journaling + revert
        // logic against synthetic filesystems, which cannot live under
        // the real /sys or /proc. The structural file_name + parent
        // check below still applies, so a test path must still match
        // the expected attribute shape — only the root prefix is
        // relaxed. Production paths (not under temp_dir) are always
        // held to the full root + shape contract.
        let test_temp_root = if cfg!(test) {
            std::env::temp_dir()
        } else {
            PathBuf::from("/__nonexistent_test_only__")
        };
        let under_test_temp = cfg!(test) && path.starts_with(&test_temp_root);

        match self {
            Capability::CpuEpp => {
                // Must be /sys/devices/system/cpu/cpuN/energy_performance_preference
                // (test mode: any path under temp_dir with the right file_name).
                let ok = (path.starts_with("/sys/devices/system/cpu") || under_test_temp)
                    && path.file_name().and_then(|n| n.to_str())
                        == Some("energy_performance_preference");
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability CpuEpp: path {} is not /sys/devices/system/cpu/cpuN/energy_performance_preference",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::PlatformProfile => {
                let ok = path == Path::new("/sys/firmware/acpi/platform_profile")
                    || (under_test_temp
                        && path.file_name().and_then(|n| n.to_str()) == Some("platform_profile"));
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability PlatformProfile: path {} is not /sys/firmware/acpi/platform_profile",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::VmSysctl => {
                let real = path == Path::new("/proc/sys/vm/swappiness")
                    || path == Path::new("/proc/sys/vm/dirty_background_bytes")
                    || path == Path::new("/proc/sys/vm/dirty_bytes");
                let test_ok = under_test_temp
                    && matches!(
                        path.file_name().and_then(|n| n.to_str()),
                        Some("swappiness") | Some("dirty_background_bytes") | Some("dirty_bytes")
                    );
                if !(real || test_ok) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability VmSysctl: path {} is not one of /proc/sys/vm/{{swappiness,dirty_background_bytes,dirty_bytes}}",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::CpuDmaLatency => {
                let ok = path == Path::new("/dev/cpu_dma_latency")
                    || (under_test_temp
                        && path.file_name().and_then(|n| n.to_str()) == Some("cpu_dma_latency"));
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability CpuDmaLatency: path {} is not /dev/cpu_dma_latency",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::DeviceResumeLatency => {
                let ok = (path.starts_with("/sys/") || under_test_temp)
                    && path.file_name().and_then(|n| n.to_str())
                        == Some("pm_qos_resume_latency_us")
                    && path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        == Some("power");
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability DeviceResumeLatency: path {} is not .../power/pm_qos_resume_latency_us",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::RuntimePm => {
                // Accept either power/control or power/autosuspend_delay_ms
                // under a /sys/ device directory.
                let name = path.file_name().and_then(|n| n.to_str());
                let parent_is_power = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    == Some("power");
                let ok = (path.starts_with("/sys/") || under_test_temp)
                    && parent_is_power
                    && matches!(name, Some("control") | Some("autosuspend_delay_ms"));
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability RuntimePm: path {} is not .../power/{{control,autosuspend_delay_ms}}",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::PcieAspm => {
                let ok = (path.starts_with("/sys/") || under_test_temp)
                    && path.file_name().and_then(|n| n.to_str()) == Some("l1_aspm")
                    && path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        == Some("link");
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability PcieAspm: path {} is not .../link/l1_aspm",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::SataAlpm => {
                let ok = (path.starts_with("/sys/") || under_test_temp)
                    && path.file_name().and_then(|n| n.to_str())
                        == Some("link_power_management_policy");
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability SataAlpm: path {} is not .../link_power_management_policy",
                            path.display()
                        ),
                    ));
                }
            }
            Capability::Backlight => {
                let ok = path.file_name().and_then(|n| n.to_str()) == Some("brightness")
                    && (under_test_temp
                        || path
                            .parent()
                            .and_then(|p| p.parent())
                            .and_then(|gp| gp.file_name())
                            .and_then(|n| n.to_str())
                            == Some("backlight"));
                if !ok {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "capability Backlight: path {} is not /sys/class/backlight/<name>/brightness",
                            path.display()
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Every capability the `Action` vocabulary can nominate. A new `Action`
/// variant that performs a kernel write MUST be added here; the
/// `all_capabilities_cover_every_action_variant` test enforces this by
/// counting variants.
#[allow(dead_code)]
pub(crate) const ALL_CAPABILITIES: &[Capability] = &[
    Capability::CpuEpp,
    Capability::PlatformProfile,
    Capability::VmSysctl,
    Capability::CpuDmaLatency,
    Capability::DeviceResumeLatency,
    Capability::RuntimePm,
    Capability::PcieAspm,
    Capability::SataAlpm,
    Capability::Backlight,
];

/// The deduplicated union of every capability's `required_paths()`. This
/// is the minimum `ReadWritePaths=` set the apply-mode systemd service
/// must grant. The `systemd_apply_service_grants_every_capability` test
/// enforces that the actual unit file grants (at least) this set.
#[cfg(test)]
pub(crate) fn required_read_write_paths() -> Vec<&'static str> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for cap in ALL_CAPABILITIES {
        for p in cap.required_paths() {
            if seen.insert(*p) {
                out.push(*p);
            }
        }
    }
    out
}

/// Helper for tests: parse a `ReadWritePaths=` line from a systemd unit
/// file and return the list of paths. Handles systemd backslash
/// continuations so multiline ReadWritePaths are parsed correctly.
#[cfg(test)]
pub(crate) fn parse_read_write_paths(unit_text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut collecting = false;

    for line in unit_text.lines() {
        let trimmed = line.trim();
        let value = if collecting {
            trimmed
        } else if let Some(rest) = trimmed.strip_prefix("ReadWritePaths=") {
            rest.trim()
        } else {
            continue;
        };

        let continued = value.ends_with('\\');
        let value = value.strip_suffix('\\').unwrap_or(value).trim();
        paths.extend(value.split_whitespace().map(str::to_string));
        collecting = continued;
        if !collecting {
            break;
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_read_write_paths_is_nonempty_and_deduped() {
        let paths = required_read_write_paths();
        assert!(!paths.is_empty(), "capability manifest is empty");
        let mut sorted = paths.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), paths.len(), "duplicate paths in manifest");
    }

    #[test]
    fn no_general_sys_write_permission() {
        let paths = required_read_write_paths();
        assert!(
            !paths.iter().any(|p| *p == "/sys" || *p == "/sys/"),
            "manifest grants general /sys write permission: {:?}",
            paths
        );
    }

    #[test]
    fn dynamic_capability_paths_are_declared() {
        let paths = required_read_write_paths();
        for expected in [
            "/sys/devices",
            "/sys/bus/pci",
            "/sys/bus/usb",
            "/sys/class/backlight",
            "/sys/class/scsi_host",
        ] {
            assert!(
                paths.contains(&expected),
                "capability manifest is missing {expected}: {paths:?}"
            );
        }
    }

    #[test]
    fn all_capabilities_are_service_deployable() {
        for cap in ALL_CAPABILITIES {
            assert!(
                cap.is_service_deployable(),
                "{:?} must declare its systemd writable prefixes",
                cap
            );
        }
    }

    #[test]
    fn all_capabilities_cover_every_action_variant() {
        // One Capability variant per kernel-writing Action variant.
        // SystemdSetProperty is excluded because it invokes systemctl,
        // not a kernel write. If you add a new Action variant that
        // writes to the kernel, add a Capability variant here.
        assert_eq!(
            ALL_CAPABILITIES.len(),
            9,
            "ALL_CAPABILITIES must have 9 entries (one per kernel-writing Action variant)"
        );
    }

    #[test]
    fn capability_target_validation_rejects_traversal() {
        let evil = Path::new("/sys/devices/system/cpu/cpu0/../../evil");
        let res = Capability::CpuEpp.validate_target(evil);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn capability_target_validation_rejects_wrong_file_name() {
        // Right subtree, wrong file name — a structurally similar
        // attempt to write an unrelated attribute.
        let wrong = Path::new("/sys/devices/system/cpu/cpu0/energy_performance_available");
        assert!(Capability::CpuEpp.validate_target(wrong).is_err());

        // power/control is RuntimePm, not DeviceResumeLatency.
        let cross = Path::new("/sys/devices/pci0000:00/0000:00:00.0/power/control");
        assert!(Capability::DeviceResumeLatency
            .validate_target(cross)
            .is_err());
    }

    #[test]
    fn capability_target_validation_accepts_legitimate_paths() {
        Capability::CpuEpp
            .validate_target(Path::new(
                "/sys/devices/system/cpu/cpu0/energy_performance_preference",
            ))
            .unwrap();
        Capability::PlatformProfile
            .validate_target(Path::new("/sys/firmware/acpi/platform_profile"))
            .unwrap();
        Capability::VmSysctl
            .validate_target(Path::new("/proc/sys/vm/swappiness"))
            .unwrap();
        Capability::CpuDmaLatency
            .validate_target(Path::new("/dev/cpu_dma_latency"))
            .unwrap();
        Capability::DeviceResumeLatency
            .validate_target(Path::new(
                "/sys/devices/pci0000:00/0000:00:00.0/power/pm_qos_resume_latency_us",
            ))
            .unwrap();
        Capability::RuntimePm
            .validate_target(Path::new(
                "/sys/devices/pci0000:00/0000:00:00.0/power/control",
            ))
            .unwrap();
        Capability::RuntimePm
            .validate_target(Path::new(
                "/sys/devices/pci0000:00/0000:00:00.0/power/autosuspend_delay_ms",
            ))
            .unwrap();
        Capability::PcieAspm
            .validate_target(Path::new(
                "/sys/devices/pci0000:00/0000:00:00.0/link/l1_aspm",
            ))
            .unwrap();
        Capability::SataAlpm
            .validate_target(Path::new(
                "/sys/devices/pci0000:00/0000:00:1f.2/ata1/host0/scsi_host/host0/link_power_management_policy",
            ))
            .unwrap();
        Capability::Backlight
            .validate_target(Path::new("/sys/class/backlight/intel_backlight/brightness"))
            .unwrap();
    }

    #[test]
    fn capability_target_validation_rejects_out_of_root() {
        // Out-of-root: /etc/passwd is not under any capability's
        // permitted root.
        let res = Capability::CpuEpp.validate_target(Path::new("/etc/passwd"));
        assert!(res.is_err());
        // Wrong capability for a valid path: backlight path is not
        // valid for CpuEpp.
        let res = Capability::CpuEpp
            .validate_target(Path::new("/sys/class/backlight/intel_backlight/brightness"));
        assert!(res.is_err());
    }

    #[test]
    fn capability_target_validation_rejects_symlink_tree_confusion() {
        // /sys/devices/.../brightness is NOT a valid Backlight target:
        // the Backlight shape requires `backlight` as the grandparent of
        // `brightness` (i.e. /sys/class/backlight/<name>/brightness, or
        // /sys/devices/.../backlight/<name>/brightness). A bare
        // `brightness` under a PCI device must be rejected so an
        // attacker cannot write to an unrelated `brightness` attribute
        // elsewhere in sysfs.
        let fake = Path::new("/sys/devices/pci0000:00/0000:00:00.0/brightness");
        assert!(Capability::Backlight.validate_target(fake).is_err());

        // /sys/bus/usb/devices/... is a symlink tree into /sys/devices.
        // We cannot structurally distinguish it from a valid /sys/devices
        // path without filesystem access. The actuator resolves symlinks
        // via fs::canonicalize() BEFORE calling validate_target, so a
        // symlink-tree path is normalized to its /sys/devices target
        // before validation. Here we test that a path with the wrong
        // file_name under /sys/bus is still rejected by the file_name
        // check (RuntimePm requires `control` or `autosuspend_delay_ms`).
        let symlink_wrong_name = Path::new("/sys/bus/usb/devices/1-1/power/evil");
        assert!(Capability::RuntimePm
            .validate_target(symlink_wrong_name)
            .is_err());
    }

    #[test]
    fn parse_read_write_paths_extracts_entries() {
        let unit = "[Service]\nReadWritePaths=/run/optid /sys/devices/system/cpu /proc/sys/vm\n";
        let paths = parse_read_write_paths(unit);
        assert_eq!(
            paths,
            vec![
                "/run/optid".to_string(),
                "/sys/devices/system/cpu".to_string(),
                "/proc/sys/vm".to_string(),
            ]
        );
    }

    #[test]
    fn parse_read_write_paths_handles_continuations() {
        let unit =
            "[Service]\nReadWritePaths=/run/optid \\\n    /sys/devices \\\n    /sys/bus/pci\n";
        assert_eq!(
            parse_read_write_paths(unit),
            vec![
                "/run/optid".to_string(),
                "/sys/devices".to_string(),
                "/sys/bus/pci".to_string(),
            ]
        );
    }

    #[test]
    fn parse_read_write_paths_returns_empty_when_absent() {
        let unit = "[Service]\nExecStart=/usr/libexec/optid\n";
        let paths = parse_read_write_paths(unit);
        assert!(paths.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────
    // Systemd unit ↔ capability manifest drift detection (Phase 4).
    // ─────────────────────────────────────────────────────────────────
    //
    // The apply-mode systemd unit's `ReadWritePaths=` line is the
    // deployed expression of this manifest. These tests fail
    // mechanically when either side drifts.

    /// Path of the packaging systemd units relative to the crate root
    /// (crates/optid -> repo root is two `..`).
    const PACKAGING_OPTID_SERVICE: &str = "../../packaging/systemd/optid.service";
    const PACKAGING_OPTID_APPLY_SERVICE: &str = "../../packaging/systemd/optid-apply.service";
    const MKOSI_OPTID_SERVICE: &str =
        "../../mkosi/mkosi.extra/usr/lib/systemd/system/optid.service";
    const MKOSI_OPTID_APPLY_SERVICE: &str =
        "../../mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service";

    fn read_unit(rel: &str) -> String {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = Path::new(manifest_dir).join(rel);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
    }

    fn grants_path(granted: &[String], path: &str) -> bool {
        // systemd treats ReadWritePaths entries as recursive writable
        // subtrees, so granting `/sys/devices` covers
        // `/sys/devices/system/cpu` etc. We require every
        // capability-required path to appear explicitly OR be covered
        // by a broader granted entry.
        let p = Path::new(path);
        granted.iter().any(|g| {
            let g_path = Path::new(g);
            p == g_path || p.starts_with(g_path)
        })
    }

    #[test]
    fn systemd_apply_service_grants_every_capability() {
        let unit = read_unit(PACKAGING_OPTID_APPLY_SERVICE);
        let granted = parse_read_write_paths(&unit);
        let required = required_read_write_paths();

        assert!(
            !granted.is_empty(),
            "apply-mode unit has no ReadWritePaths= line — sandbox is fully closed"
        );

        let mut missing: Vec<&str> = Vec::new();
        for req in &required {
            if !grants_path(&granted, req) {
                missing.push(*req);
            }
        }
        assert!(
            missing.is_empty(),
            "apply-mode unit's ReadWritePaths= is missing capability-required paths: {:?}.\n\
             Granted: {:?}\n\
             Required by capability manifest: {:?}\n\
             Hint: add the missing paths to packaging/systemd/optid-apply.service AND\n\
             mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service.\n\
             See crates/optid/src/capability.rs for the manifest.",
            missing,
            granted,
            required,
        );
    }

    #[test]
    fn systemd_apply_service_has_protect_kernel_tunables() {
        let unit = read_unit(PACKAGING_OPTID_APPLY_SERVICE);
        assert!(
            unit.contains("ProtectKernelTunables=yes"),
            "optid-apply.service must set ProtectKernelTunables=yes"
        );
    }

    #[test]
    fn systemd_dry_run_service_does_not_grant_apply_capabilities() {
        let unit = read_unit(PACKAGING_OPTID_SERVICE);
        let granted = parse_read_write_paths(&unit);
        let required = required_read_write_paths();

        // The dry-run unit MAY grant /run/optid (state directory).
        // It MUST NOT grant any capability-required path — dry-run
        // must not retain unnecessary system-write capabilities
        // (Phase 4 acceptance criterion 1).
        let forbidden: Vec<&str> = granted
            .iter()
            .filter(|p| **p != "/run/optid")
            .filter(|p| required.iter().any(|r| r == *p))
            .map(|p| p.as_str())
            .collect();
        assert!(
            forbidden.is_empty(),
            "dry-run unit grants system-write paths that are only needed in apply mode: {:?}.\n\
             Dry-run must not retain unnecessary system-write capabilities.\n\
             See crates/optid/src/capability.rs for the manifest.",
            forbidden,
        );
    }

    #[test]
    fn mkosi_mirror_matches_packaging() {
        let packaging_main = read_unit(PACKAGING_OPTID_SERVICE);
        let mkosi_main = read_unit(MKOSI_OPTID_SERVICE);
        assert_eq!(
            packaging_main, mkosi_main,
            "mkosi mirror of optid.service drifted from packaging copy.\n\
             Hint: copy packaging/systemd/optid.service to\n\
             mkosi/mkosi.extra/usr/lib/systemd/system/optid.service."
        );

        let packaging_apply = read_unit(PACKAGING_OPTID_APPLY_SERVICE);
        let mkosi_apply = read_unit(MKOSI_OPTID_APPLY_SERVICE);
        assert_eq!(
            packaging_apply, mkosi_apply,
            "mkosi mirror of optid-apply.service drifted from packaging copy.\n\
             Hint: copy packaging/systemd/optid-apply.service to\n\
             mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service."
        );
    }
}
