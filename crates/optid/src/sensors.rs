//! Pressure Stall Information (PSI) and live system snapshot collection.
//!
//! This module owns every "read from sysfs/procfs" path used by `optid`. The
//! `Snapshot` struct is the immutable per-loop observation that the policy
//! engine reasons about. Keeping the readers in one place makes it obvious
//! which kernel interfaces the daemon depends on and makes it easy to mock
//! the world in tests by constructing `Snapshot` literals.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::workload::WorkloadClass;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Pressure {
    pub(crate) avg10: f32,
    pub(crate) avg60: f32,
    pub(crate) avg300: f32,
    pub(crate) total: u64,
}

impl Pressure {
    pub(crate) fn read(path: &str) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        parse_pressure(&text)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub(crate) timestamp: u64,
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
}

impl Snapshot {
    pub(crate) fn collect() -> Self {
        Self {
            timestamp: now_unix(),
            on_ac: read_on_ac(),
            battery_pct: read_battery_pct(),
            max_temp_millic: read_max_thermal_millic(),
            loadavg_1: read_loadavg_1(),
            cpu_pressure: Pressure::read("/proc/pressure/cpu"),
            memory_pressure: Pressure::read("/proc/pressure/memory"),
            io_pressure: Pressure::read("/proc/pressure/io"),
            zram_swap_active: read_zram_swap_active(),
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: discover_pm_qos_device_paths(),
            runtime_pm_device_paths: discover_runtime_pm_device_paths(),
            pcie_aspm_device_paths: discover_pcie_aspm_device_paths(),
            sata_alpm_host_paths: discover_sata_alpm_host_paths(),
            selected_backlight: crate::actuators::display::select_backlight(
                &discover_backlight_devices(),
            ),
            is_vm_guest: detect_vm_guest(),
        }
    }

    pub(crate) fn thermal_c(&self) -> Option<f32> {
        self.max_temp_millic.map(|temp| temp as f32 / 1000.0)
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

pub(crate) fn discover_pm_qos_device_paths() -> Vec<PathBuf> {
    let base = Path::new("/sys/bus/pci/devices");
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(base) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path().join("power").join("pm_qos_resume_latency_us");
        if path.exists() {
            paths.push(path);
        }
    }
    paths
}

/// Enumerate runtime-PM-capable device directories: any device under
/// `/sys/bus/{pci,usb}/devices/` that exposes a writable `power/control`
/// attribute. The list is intentionally broad — the actuator narrows it to the
/// allowlisted set. Non-blocking directory reads only (per the optid invariant).
pub(crate) fn discover_runtime_pm_device_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for bus in ["/sys/bus/pci/devices", "/sys/bus/usb/devices"] {
        let Ok(entries) = fs::read_dir(bus) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let dev = entry.path();
            if dev.join("power").join("control").exists() {
                paths.push(dev);
            }
        }
    }
    paths
}

/// Enumerate PCI devices exposing `link/l1_aspm` (kernel ≥ 5.2) — WP-N6 PCIe
/// ASPM candidates. The actuator narrows this to the allowlisted set.
pub(crate) fn discover_pcie_aspm_device_paths() -> Vec<PathBuf> {
    let base = Path::new("/sys/bus/pci/devices");
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(base) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let dev = entry.path();
        if dev.join("link").join("l1_aspm").exists() {
            paths.push(dev);
        }
    }
    paths
}

/// Enumerate SCSI hosts exposing `link_power_management_policy` — WP-N6 SATA
/// ALPM candidates.
pub(crate) fn discover_sata_alpm_host_paths() -> Vec<PathBuf> {
    let base = Path::new("/sys/class/scsi_host");
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(base) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let host = entry.path();
        if host.join("link_power_management_policy").exists() {
            paths.push(host);
        }
    }
    paths
}

/// Enumerate backlight device directories under `/sys/class/backlight/` —
/// WP-N7 candidates. Selection among them is `display::select_backlight`.
pub(crate) fn discover_backlight_devices() -> Vec<PathBuf> {
    let base = Path::new("/sys/class/backlight");
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(base) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let dev = entry.path();
        if dev.join("brightness").exists() {
            paths.push(dev);
        }
    }
    paths
}

pub(crate) fn discover_cpu_epp_paths() -> Vec<PathBuf> {
    let base = Path::new("/sys/devices/system/cpu");
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("cpu") && name[3..].chars().all(|ch| ch.is_ascii_digit())
                })
        })
        .map(|path| path.join("cpufreq/energy_performance_preference"))
        .filter(|path| path.exists())
        .collect()
}

pub(crate) fn read_on_ac() -> Option<bool> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    let mut saw_battery = false;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
        let kind = kind.trim();
        if kind.eq_ignore_ascii_case("Battery") {
            saw_battery = true;
            continue;
        }

        if matches!(kind, "Mains" | "USB" | "USB_C" | "USB_PD") {
            if let Ok(online) = fs::read_to_string(path.join("online")) {
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

pub(crate) fn read_battery_pct() -> Option<u8> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
        if kind.trim().eq_ignore_ascii_case("Battery") {
            let capacity = fs::read_to_string(path.join("capacity")).ok()?;
            return capacity.trim().parse::<u8>().ok();
        }
    }
    None
}

pub(crate) fn read_zram_swap_active() -> bool {
    #[cfg(test)]
    if let Ok(val) = std::env::var("OPTID_MOCK_ZRAM_SWAP_ACTIVE") {
        return val == "true";
    }

    let Ok(text) = fs::read_to_string("/proc/swaps") else {
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
pub(crate) fn detect_vm_guest() -> bool {
    let Ok(text) = fs::read_to_string("/sys/class/dmi/id/sys_vendor") else {
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

pub(crate) fn read_max_thermal_millic() -> Option<i64> {
    let entries = fs::read_dir("/sys/class/thermal").ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("thermal_zone"))
        })
        .filter_map(|entry| fs::read_to_string(entry.path().join("temp")).ok())
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .max()
}

pub(crate) fn read_loadavg_1() -> Option<f32> {
    let text = fs::read_to_string("/proc/loadavg").ok()?;
    text.split_whitespace().next()?.parse::<f32>().ok()
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
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
}
