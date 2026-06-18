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
}
