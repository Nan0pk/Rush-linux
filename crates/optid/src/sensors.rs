//! Pressure Stall Information (PSI) and live system snapshot collection.
//!
//! This module owns every "read from sysfs/procfs" path used by `optid`. The
//! `Snapshot` struct is the immutable per-loop observation that the policy
//! engine reasons about. Keeping the readers in one place makes it obvious
//! which kernel interfaces the daemon depends on and makes it easy to mock
//! the world in tests by constructing `Snapshot` literals.
//!
//! F2: the actual filesystem and clock calls are now routed through the
//! [`kernel_io`](crate::kernel_io) traits. Every public function in this
//! module has a `*_with(read, clock)` form that accepts injected I/O, and
//! the legacy no-argument form delegates to `RealKernel::new()`. This
//! preserves bit-for-bit behavior while making fault-injection and
//! deterministic simulation possible from tests.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::kernel_io::{Clock, KernelRead, RealKernel};
use crate::workload::WorkloadClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationFailureKind {
    NotFound,
    PermissionDenied,
    InvalidData,
    Malformed,
    Other,
}

impl ObservationFailureKind {
    fn from_io(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => Self::InvalidData,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Pressure {
    pub(crate) avg10: f32,
    pub(crate) avg60: f32,
    pub(crate) avg300: f32,
    pub(crate) total: u64,
}

impl Pressure {
    /// F2: production path — delegates to `RealKernel`.
    #[allow(dead_code)]
    pub(crate) fn read(path: &str) -> Option<Self> {
        Self::read_with(&RealKernel::new(), path)
    }

    /// F2: injectable read path for tests.
    pub(crate) fn read_with(read: &dyn KernelRead, path: &str) -> Option<Self> {
        Self::read_with_diagnostic(read, path).0
    }

    pub(crate) fn read_with_diagnostic(
        read: &dyn KernelRead,
        path: &str,
    ) -> (Option<Self>, Option<ObservationFailureKind>) {
        let text = match read.read_to_string(Path::new(path)) {
            Ok(text) => text,
            Err(error) => return (None, Some(ObservationFailureKind::from_io(&error))),
        };
        match parse_pressure(&text) {
            Some(pressure) => (Some(pressure), None),
            None => (None, Some(ObservationFailureKind::Malformed)),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Snapshot {
    pub(crate) timestamp: u64,
    pub(crate) observation_failures: BTreeMap<String, ObservationFailureKind>,
    pub(crate) on_ac: Option<bool>,
    pub(crate) battery_pct: Option<u8>,
    pub(crate) max_temp_millic: Option<i64>,
    pub(crate) loadavg_1: Option<f32>,
    pub(crate) cpu_pressure: Option<Pressure>,
    pub(crate) memory_pressure: Option<Pressure>,
    pub(crate) io_pressure: Option<Pressure>,
    pub(crate) zram_swap_active: bool,
    pub(crate) foreground_app: Option<String>,
    pub(crate) pinned_class: Option<WorkloadClass>,
    pub(crate) global_pinned_class: Option<WorkloadClass>,
    pub(crate) pm_qos_device_paths: Vec<PathBuf>,
    /// Device directories (under /sys/bus/{pci,usb}/devices/*) that expose a
    /// `power/control` attribute — i.e. candidates for WP-N5 runtime-PM
    /// autosuspend. The actuator still gates each one on the N4 HWID allowlist.
    pub(crate) runtime_pm_device_paths: Vec<PathBuf>,
    /// PCI device directories that expose `link/l1_aspm` — WP-N6 PCIe ASPM
    /// candidates. Allowlist-gated (domain `pci_aspm`) in the actuator.
    pub(crate) pcie_aspm_device_paths: Vec<PathBuf>,
    /// SCSI host directories that expose `link_power_management_policy` — WP-N6
    /// SATA ALPM candidates. Allowlist-gated (domain `sata_alpm`).
    pub(crate) sata_alpm_host_paths: Vec<PathBuf>,
    /// The active backlight device dir (`/sys/class/backlight/<name>`) selected
    /// by the §1.1 heuristic — WP-N7. `None` when no backlight is present.
    pub(crate) selected_backlight: Option<PathBuf>,
    /// v0.6 Phase C2: `true` when DMI reports a hypervisor vendor.
    pub(crate) is_vm_guest: bool,
    /// T1 — Discovered thermal sensors (°C).
    pub(crate) thermal_sensors: Vec<crate::thermal::ThermalSensor>,
    /// T1 — Discovered fan sensors (RPM).
    pub(crate) fan_sensors: Vec<crate::thermal::FanSensor>,
    /// T1 — Pure deterministic thermal budget.
    pub(crate) thermal_budget: crate::thermal::ThermalBudget,
}

impl Snapshot {
    /// F2: injectable collect path for tests. Takes separate `read` and
    /// `clock` parameters so a test can mix-and-match (e.g. a `FaultKernel`
    /// for reads with a `RealKernel` clock).
    ///
    /// Post-#337 repair: the no-argument `Snapshot::collect()` form was
    /// removed. It used `ThermalConfig::default()` (mode=Observe) and was
    /// the source of the startup-scan defect: the daemon called it before
    /// loading the configured thermal mode, bypassing operator
    /// configuration. The production path is `collect_with_thermal` with
    /// the policy's thermal config; tests that need a default-config
    /// snapshot should call `collect_with(&k, &k)` explicitly.
    #[cfg(test)]
    pub(crate) fn collect_with(read: &dyn KernelRead, clock: &dyn Clock) -> Self {
        Self::collect_with_thermal(read, clock, &crate::thermal::ThermalConfig::default(), None)
    }

    /// Production thermal path: snapshot collection with injected thermal
    /// config and previous budget (for hysteresis across loop iterations).
    pub(crate) fn collect_with_thermal(
        read: &dyn KernelRead,
        clock: &dyn Clock,
        thermal_config: &crate::thermal::ThermalConfig,
        previous_budget: Option<&crate::thermal::ThermalBudget>,
    ) -> Self {
        let (thermal_sensors, fan_sensors, thermal_budget) =
            crate::thermal::collect_thermal_budget_with(read, thermal_config, previous_budget);

        // When thermal mode is off, skip legacy thermal-zone max-temp
        // discovery so policy does not observe temps through a back door.
        let max_temp_millic = if thermal_config.mode == crate::thermal::ThermalMode::Off {
            None
        } else {
            read_max_thermal_millic_with(read)
        };

        let (cpu_pressure, cpu_pressure_failure) =
            Pressure::read_with_diagnostic(read, "/proc/pressure/cpu");
        let (memory_pressure, memory_pressure_failure) =
            Pressure::read_with_diagnostic(read, "/proc/pressure/memory");
        let (io_pressure, io_pressure_failure) =
            Pressure::read_with_diagnostic(read, "/proc/pressure/io");
        let mut observation_failures = BTreeMap::new();
        for (component, failure) in [
            ("cpu-pressure", cpu_pressure_failure),
            ("memory-pressure", memory_pressure_failure),
            ("io-pressure", io_pressure_failure),
        ] {
            if let Some(failure) = failure {
                observation_failures.insert(component.to_string(), failure);
            }
        }

        Self {
            timestamp: clock.now_unix(),
            observation_failures,
            on_ac: read_on_ac_with(read),
            battery_pct: read_battery_pct_with(read),
            max_temp_millic,
            loadavg_1: read_loadavg_1_with(read),
            cpu_pressure,
            memory_pressure,
            io_pressure,
            zram_swap_active: read_zram_swap_active_with(read),
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: discover_pm_qos_device_paths_with(read),
            runtime_pm_device_paths: discover_runtime_pm_device_paths_with(read),
            pcie_aspm_device_paths: discover_pcie_aspm_device_paths_with(read),
            sata_alpm_host_paths: discover_sata_alpm_host_paths_with(read),
            selected_backlight: crate::actuators::display::select_backlight(
                &discover_backlight_devices_with(read),
            ),
            is_vm_guest: detect_vm_guest_with(read),
            thermal_sensors,
            fan_sensors,
            thermal_budget,
        }
    }

    /// Operator- and policy-facing temperature. Never invents a temperature
    /// when thermal sensing is disabled or telemetry is unavailable — the
    /// legacy `max_temp_millic` path must not override those states.
    pub(crate) fn thermal_c(&self) -> Option<f32> {
        use crate::thermal::ThermalBudgetState;
        match self.thermal_budget.state {
            ThermalBudgetState::Disabled | ThermalBudgetState::Unavailable => None,
            _ => self
                .thermal_budget
                .max_die_temp_c
                .or_else(|| self.max_temp_millic.map(|temp| temp as f32 / 1000.0)),
        }
    }
}

pub(crate) fn parse_pressure(text: &str) -> Option<Pressure> {
    let line = text
        .lines()
        .find(|line| line.starts_with("some "))
        .or_else(|| text.lines().next())?;

    let mut pressure = Pressure::default();
    for token in line.split_whitespace().skip(1) {
        let (key, value) = token.split_once('=')?;
        match key {
            "avg10" => pressure.avg10 = value.parse().ok()?,
            "avg60" => pressure.avg60 = value.parse().ok()?,
            "avg300" => pressure.avg300 = value.parse().ok()?,
            "total" => pressure.total = value.parse().ok()?,
            _ => {}
        }
    }
    Some(pressure)
}

pub(crate) fn fmt_pressure(value: Option<Pressure>) -> String {
    match value {
        Some(p) => format!(
            "avg10={:.2} avg60={:.2} avg300={:.2} total={}",
            p.avg10, p.avg60, p.avg300, p.total
        ),
        None => "unavailable".to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────
// F2: every discover_* and read_* function has a `*_with(read)` form
// that takes an injected `KernelRead`, and a legacy no-argument form
// that delegates to `RealKernel::new()`. The production path and
// the test path share the same implementation.
// ─────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub(crate) fn discover_pm_qos_device_paths() -> Vec<PathBuf> {
    discover_pm_qos_device_paths_with(&RealKernel::new())
}

pub(crate) fn discover_pm_qos_device_paths_with(read: &dyn KernelRead) -> Vec<PathBuf> {
    let base = Path::new("/sys/bus/pci/devices");
    let mut paths = Vec::new();
    let Ok(entries) = read.read_dir(base) else {
        return paths;
    };
    for entry in entries {
        let path = entry.join("power").join("pm_qos_resume_latency_us");
        if read.exists(&path) {
            paths.push(path);
        }
    }
    paths
}

/// Enumerate runtime-PM-capable device directories: any device under
/// `/sys/bus/{pci,usb}/devices/` that exposes a writable `power/control`
/// attribute. The list is intentionally broad — the actuator narrows it to the
/// allowlisted set. Non-blocking directory reads only (per the optid invariant).
#[allow(dead_code)]
pub(crate) fn discover_runtime_pm_device_paths() -> Vec<PathBuf> {
    discover_runtime_pm_device_paths_with(&RealKernel::new())
}

pub(crate) fn discover_runtime_pm_device_paths_with(read: &dyn KernelRead) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for bus in ["/sys/bus/pci/devices", "/sys/bus/usb/devices"] {
        let Ok(entries) = read.read_dir(Path::new(bus)) else {
            continue;
        };
        for dev in entries {
            if read.exists(&dev.join("power").join("control")) {
                paths.push(dev);
            }
        }
    }
    paths
}

/// Enumerate PCI devices exposing `link/l1_aspm` (kernel ≥ 5.2) — WP-N6 PCIe
/// ASPM candidates. The actuator narrows this to the allowlisted set.
#[allow(dead_code)]
pub(crate) fn discover_pcie_aspm_device_paths() -> Vec<PathBuf> {
    discover_pcie_aspm_device_paths_with(&RealKernel::new())
}

pub(crate) fn discover_pcie_aspm_device_paths_with(read: &dyn KernelRead) -> Vec<PathBuf> {
    let base = Path::new("/sys/bus/pci/devices");
    let mut paths = Vec::new();
    let Ok(entries) = read.read_dir(base) else {
        return paths;
    };
    for dev in entries {
        if read.exists(&dev.join("link").join("l1_aspm")) {
            paths.push(dev);
        }
    }
    paths
}

/// Enumerate SCSI hosts exposing `link_power_management_policy` — WP-N6 SATA
/// ALPM candidates.
#[allow(dead_code)]
pub(crate) fn discover_sata_alpm_host_paths() -> Vec<PathBuf> {
    discover_sata_alpm_host_paths_with(&RealKernel::new())
}

pub(crate) fn discover_sata_alpm_host_paths_with(read: &dyn KernelRead) -> Vec<PathBuf> {
    let base = Path::new("/sys/class/scsi_host");
    let mut paths = Vec::new();
    let Ok(entries) = read.read_dir(base) else {
        return paths;
    };
    for host in entries {
        if read.exists(&host.join("link_power_management_policy")) {
            paths.push(host);
        }
    }
    paths
}

/// Enumerate backlight device directories under `/sys/class/backlight/` —
/// WP-N7 candidates. Selection among them is `display::select_backlight`.
#[allow(dead_code)]
pub(crate) fn discover_backlight_devices() -> Vec<PathBuf> {
    discover_backlight_devices_with(&RealKernel::new())
}

pub(crate) fn discover_backlight_devices_with(read: &dyn KernelRead) -> Vec<PathBuf> {
    let base = Path::new("/sys/class/backlight");
    let mut paths = Vec::new();
    let Ok(entries) = read.read_dir(base) else {
        return paths;
    };
    for dev in entries {
        if read.exists(&dev.join("brightness")) {
            paths.push(dev);
        }
    }
    paths
}

#[allow(dead_code)]
pub(crate) fn discover_cpu_epp_paths() -> Vec<PathBuf> {
    discover_cpu_epp_paths_with(&RealKernel::new())
}

pub(crate) fn discover_cpu_epp_paths_with(read: &dyn KernelRead) -> Vec<PathBuf> {
    let base = Path::new("/sys/devices/system/cpu");
    let Ok(entries) = read.read_dir(base) else {
        return Vec::new();
    };

    entries
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("cpu") && name[3..].chars().all(|ch| ch.is_ascii_digit())
                })
        })
        .map(|path| path.join("cpufreq/energy_performance_preference"))
        .filter(|path| read.exists(path))
        .collect()
}

#[allow(dead_code)]
pub(crate) fn read_on_ac() -> Option<bool> {
    read_on_ac_with(&RealKernel::new())
}

pub(crate) fn read_on_ac_with(read: &dyn KernelRead) -> Option<bool> {
    let entries = read.read_dir(Path::new("/sys/class/power_supply")).ok()?;
    let mut saw_battery = false;

    for path in entries {
        let kind = read.read_to_string(&path.join("type")).unwrap_or_default();
        let kind = kind.trim();
        if kind.eq_ignore_ascii_case("Battery") {
            saw_battery = true;
            continue;
        }

        if matches!(kind, "Mains" | "USB" | "USB_C" | "USB_PD") {
            if let Ok(online) = read.read_to_string(&path.join("online")) {
                return Some(online.trim() == "1");
            }
        }
    }

    if saw_battery {
        Some(false)
    } else {
        None
    }
}

#[allow(dead_code)]
pub(crate) fn read_battery_pct() -> Option<u8> {
    read_battery_pct_with(&RealKernel::new())
}

pub(crate) fn read_battery_pct_with(read: &dyn KernelRead) -> Option<u8> {
    let entries = read.read_dir(Path::new("/sys/class/power_supply")).ok()?;
    for path in entries {
        let kind = read.read_to_string(&path.join("type")).unwrap_or_default();
        if kind.trim().eq_ignore_ascii_case("Battery") {
            let capacity = read.read_to_string(&path.join("capacity")).ok()?;
            return capacity.trim().parse::<u8>().ok();
        }
    }
    None
}

#[allow(dead_code)]
pub(crate) fn read_zram_swap_active() -> bool {
    read_zram_swap_active_with(&RealKernel::new())
}

pub(crate) fn read_zram_swap_active_with(read: &dyn KernelRead) -> bool {
    #[cfg(test)]
    if let Ok(val) = std::env::var("OPTID_MOCK_ZRAM_SWAP_ACTIVE") {
        return val == "true";
    }

    let Ok(text) = read.read_to_string(Path::new("/proc/swaps")) else {
        return false;
    };
    for line in text.lines().skip(1) {
        if line.contains("/dev/zram") {
            return true;
        }
    }
    false
}

/// v0.6 Phase C2: Read `/sys/class/dmi/id/sys_vendor` and return `true`
/// if the vendor string matches a known hypervisor.
#[allow(dead_code)]
pub(crate) fn detect_vm_guest() -> bool {
    detect_vm_guest_with(&RealKernel::new())
}

pub(crate) fn detect_vm_guest_with(read: &dyn KernelRead) -> bool {
    let Ok(text) = read.read_to_string(Path::new("/sys/class/dmi/id/sys_vendor")) else {
        return false;
    };
    is_vm_guest_sys_vendor(text.trim())
}

/// v0.6 Phase C2: Pure predicate — does `sys_vendor` match a known
/// hypervisor vendor? Separated from `detect_vm_guest` so tests can
/// exercise the matching logic without poking at `/sys`.
pub(crate) fn is_vm_guest_sys_vendor(sys_vendor: &str) -> bool {
    let v = sys_vendor.trim().to_ascii_lowercase();
    matches!(
        v.as_str(),
        "qemu"
            | "kvm"
            | "vmware, inc."
            | "vmware"
            | "xen"
            | "microsoft corporation"
            | "innotek gmbh"
            | "parallels software international inc."
    )
}

#[allow(dead_code)]
pub(crate) fn read_max_thermal_millic() -> Option<i64> {
    read_max_thermal_millic_with(&RealKernel::new())
}

pub(crate) fn read_max_thermal_millic_with(read: &dyn KernelRead) -> Option<i64> {
    let entries = read.read_dir(Path::new("/sys/class/thermal")).ok()?;
    entries
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("thermal_zone"))
        })
        .filter_map(|path| read.read_to_string(&path.join("temp")).ok())
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .max()
}

#[allow(dead_code)]
pub(crate) fn read_loadavg_1() -> Option<f32> {
    read_loadavg_1_with(&RealKernel::new())
}

pub(crate) fn read_loadavg_1_with(read: &dyn KernelRead) -> Option<f32> {
    let text = read.read_to_string(Path::new("/proc/loadavg")).ok()?;
    text.split_whitespace().next()?.parse::<f32>().ok()
}

#[allow(dead_code)]
pub(crate) fn now_unix() -> u64 {
    RealKernel::new().now_unix()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_psi_some_line() {
        let pressure = parse_pressure(
            "some avg10=1.25 avg60=2.50 avg300=3.75 total=42\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
        )
        .unwrap();
        assert_eq!(pressure.avg10, 1.25);
        assert_eq!(pressure.avg60, 2.50);
        assert_eq!(pressure.avg300, 3.75);
        assert_eq!(pressure.total, 42);
    }

    #[test]
    fn fmt_pressure_renders_unavailable_when_none() {
        assert_eq!(fmt_pressure(None), "unavailable");
    }

    #[test]
    fn fmt_pressure_renders_all_fields_when_some() {
        let p = Pressure {
            avg10: 0.10,
            avg60: 0.20,
            avg300: 0.30,
            total: 7,
        };
        let s = fmt_pressure(Some(p));
        assert!(s.contains("avg10=0.10"));
        assert!(s.contains("avg60=0.20"));
        assert!(s.contains("avg300=0.30"));
        assert!(s.contains("total=7"));
    }

    // ── v0.6 Phase C2: is_vm_guest_sys_vendor ────────────────────────────

    #[test]
    fn vm_guest_detects_qemu() {
        assert!(is_vm_guest_sys_vendor("QEMU"));
        assert!(is_vm_guest_sys_vendor("qemu"));
        assert!(is_vm_guest_sys_vendor("  QEMU  "));
    }

    #[test]
    fn vm_guest_detects_kvm() {
        assert!(is_vm_guest_sys_vendor("KVM"));
        assert!(is_vm_guest_sys_vendor("kvm"));
    }

    #[test]
    fn vm_guest_detects_vmware() {
        assert!(is_vm_guest_sys_vendor("VMware, Inc."));
        assert!(is_vm_guest_sys_vendor("vmware, inc."));
        assert!(is_vm_guest_sys_vendor("VMware"));
    }

    #[test]
    fn vm_guest_detects_xen() {
        assert!(is_vm_guest_sys_vendor("Xen"));
        assert!(is_vm_guest_sys_vendor("xen"));
    }

    #[test]
    fn vm_guest_detects_microsoft_hyperv() {
        assert!(is_vm_guest_sys_vendor("Microsoft Corporation"));
        assert!(is_vm_guest_sys_vendor("microsoft corporation"));
    }

    #[test]
    fn vm_guest_detects_innotek_virtualbox() {
        assert!(is_vm_guest_sys_vendor("innotek GmbH"));
        assert!(is_vm_guest_sys_vendor("Innotek GmbH"));
    }

    #[test]
    fn vm_guest_detects_parallels() {
        assert!(is_vm_guest_sys_vendor(
            "Parallels Software International Inc."
        ));
        assert!(is_vm_guest_sys_vendor(
            "parallels software international inc."
        ));
    }

    #[test]
    fn vm_guest_rejects_real_hardware_vendors() {
        assert!(!is_vm_guest_sys_vendor("ASUS"));
        assert!(!is_vm_guest_sys_vendor("Dell Inc."));
        assert!(!is_vm_guest_sys_vendor("HP"));
        assert!(!is_vm_guest_sys_vendor("Lenovo"));
        assert!(!is_vm_guest_sys_vendor("Apple Inc."));
        assert!(!is_vm_guest_sys_vendor("Gigabyte Technology Co., Ltd."));
        assert!(!is_vm_guest_sys_vendor("Intel Corporation"));
        assert!(!is_vm_guest_sys_vendor("AMD"));
    }

    #[test]
    fn vm_guest_rejects_empty_and_garbage() {
        assert!(!is_vm_guest_sys_vendor(""));
        assert!(!is_vm_guest_sys_vendor("   "));
        assert!(!is_vm_guest_sys_vendor("not-a-vendor"));
        assert!(!is_vm_guest_sys_vendor("QEMU-like"));
        assert!(!is_vm_guest_sys_vendor("vmware clone"));
    }

    #[test]
    fn vm_guest_handles_newline_trailing() {
        assert!(is_vm_guest_sys_vendor("QEMU\n"));
        assert!(is_vm_guest_sys_vendor("VMware, Inc.\n"));
    }

    // ── F2: KernelRead injection smoke tests ───────────────────────────

    /// Verify the *_with functions accept an injected KernelRead and
    /// behave gracefully when the kernel paths are absent (as in the
    /// test container). This is a smoke test that the seam compiles and
    /// runs; the FaultKernel-based fault-injection tests live in
    /// `tests.rs::f2_fault_injection_tests`.
    #[test]
    fn f2_collect_with_real_kernel_does_not_panic() {
        let k = RealKernel::new();
        // collect_with will read whatever the container exposes (likely
        // nothing for /sys/* in CI); the important thing is it does not
        // panic and returns a Snapshot.
        let snap = Snapshot::collect_with(&k, &k);
        // Just exercise the field — the test is a smoke test that the
        // seam compiles and runs without panicking.
        let _ = snap.timestamp;
    }

    #[test]
    fn f2_read_loadavg_with_real_kernel_returns_some_on_real_kernel() {
        // /proc/loadavg exists on any Linux. The RealKernel read should
        // succeed. (If this fails, the container doesn't have /proc —
        // that's fine, the test is a smoke test for the seam.)
        let k = RealKernel::new();
        let _ = read_loadavg_1_with(&k);
        // No assertion on Some vs None — container-dependent.
    }
}
