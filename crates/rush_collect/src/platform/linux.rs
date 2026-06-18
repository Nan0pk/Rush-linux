use std::fs;
use std::path::Path;

use crate::types::*;

// ── Hardware profile ─────────────────────────────────────────────────────────

pub fn read_cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn read_cpu_vendor() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("vendor_id"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn read_cpu_cores() -> usize {
    fs::read_to_string("/proc/cpuinfo")
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count())
        .unwrap_or(0)
}

pub fn read_ram_total_kb() -> u64 {
    parse_meminfo_field("MemTotal").unwrap_or(0)
}

pub fn read_chassis() -> String {
    // DMI chassis type codes: 8=Notebook, 9=Laptop, 10=Hand, 3=Desktop, 17=Rack, etc.
    let type_id = fs::read_to_string("/sys/class/dmi/id/chassis_type")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);

    match type_id {
        8 | 9 | 10 | 14 => "Notebook".to_string(),
        3 | 4 | 5 | 6 | 7 => "Desktop".to_string(),
        17 | 23 => "Server".to_string(),
        _ => "unknown".to_string(),
    }
}

pub fn read_battery_design_uwh() -> u64 {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return 0;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("BAT") {
            continue;
        }
        // energy_full_design is in µWh
        if let Ok(s) = fs::read_to_string(entry.path().join("energy_full_design")) {
            if let Ok(v) = s.trim().parse::<u64>() {
                return v;
            }
        }
        // charge_full_design (µAh) × voltage_min_design (µV) / 10^6 → µWh
        if let Ok(c) = fs::read_to_string(entry.path().join("charge_full_design")) {
            if let Ok(charge) = c.trim().parse::<u64>() {
                if let Ok(v) = fs::read_to_string(entry.path().join("voltage_min_design")) {
                    if let Ok(volt) = v.trim().parse::<u64>() {
                        return charge * volt / 1_000_000;
                    }
                }
                return charge;
            }
        }
    }
    0
}

pub fn read_kernel() -> String {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string()
}

// ── Energy counters ───────────────────────────────────────────────────────────

pub fn read_rapl_uj() -> Option<u64> {
    fs::read_to_string("/sys/class/powercap/intel-rapl:0/energy_uj")
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn read_battery_uwh() -> Option<u64> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_name().to_string_lossy().starts_with("BAT") {
            continue;
        }
        // energy_now is in µWh on most platforms
        if let Ok(s) = fs::read_to_string(entry.path().join("energy_now")) {
            if let Ok(v) = s.trim().parse::<u64>() {
                return Some(v);
            }
        }
    }
    None
}

pub fn read_ac_online() -> Option<bool> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if n.starts_with("AC") || n.starts_with("ACAD") || n == "ac" {
            let val = fs::read_to_string(entry.path().join("online"))
                .ok()?
                .trim()
                .to_string();
            return Some(val == "1");
        }
    }
    None
}

// ── PSI ──────────────────────────────────────────────────────────────────────

/// Read the monotonic `total=` counter (µs) from /proc/pressure/{cpu,io}.
/// Returns None on kernels < 4.20 or when unavailable.
pub fn read_psi_total_us(resource: &str) -> Option<u64> {
    let content = fs::read_to_string(format!("/proc/pressure/{}", resource)).ok()?;
    // First line: "some avg10=... avg60=... avg300=... total=<N>"
    let line = content.lines().next()?;
    line.split_whitespace()
        .find(|p| p.starts_with("total="))?
        .strip_prefix("total=")?
        .parse()
        .ok()
}

// ── Thermal ───────────────────────────────────────────────────────────────────

pub fn read_thermal() -> ThermalSnapshot {
    let mut max_mc: Option<i64> = None;
    let mut zones = 0usize;

    let Ok(entries) = fs::read_dir("/sys/class/thermal") else {
        return ThermalSnapshot { max_celsius: None, zones_read: 0 };
    };

    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("thermal_zone") {
            continue;
        }
        if let Ok(s) = fs::read_to_string(entry.path().join("temp")) {
            if let Ok(mc) = s.trim().parse::<i64>() {
                zones += 1;
                max_mc = Some(max_mc.map_or(mc, |cur: i64| cur.max(mc)));
            }
        }
    }

    ThermalSnapshot {
        max_celsius: max_mc.map(|mc| (mc as f64 / 1000.0 * 10.0).round() / 10.0),
        zones_read: zones,
    }
}

// ── CPU frequency ─────────────────────────────────────────────────────────────

pub fn read_freq() -> FreqSnapshot {
    let base = Path::new("/sys/devices/system/cpu");

    let governor = fs::read_to_string(base.join("cpu0/cpufreq/scaling_governor"))
        .ok()
        .map(|s| s.trim().to_string());

    let max_mhz = fs::read_to_string(base.join("cpu0/cpufreq/scaling_max_freq"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|khz| khz / 1000);

    // Average current frequency across all online CPUs
    let mut total_mhz: u64 = 0;
    let mut count: usize = 0;
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            let n = name.to_string_lossy();
            // Match cpu0, cpu1, … but not cpufreq, cpuidle, etc.
            if !n.starts_with("cpu")
                || !n[3..].chars().all(|c| c.is_ascii_digit())
            {
                continue;
            }
            if let Ok(s) =
                fs::read_to_string(entry.path().join("cpufreq/scaling_cur_freq"))
            {
                if let Ok(khz) = s.trim().parse::<u64>() {
                    total_mhz += khz / 1000;
                    count += 1;
                }
            }
        }
    }

    FreqSnapshot {
        governor,
        max_mhz,
        current_mhz_avg: if count > 0 { Some(total_mhz / count as u64) } else { None },
    }
}

// ── Memory & load ─────────────────────────────────────────────────────────────

pub fn read_memory() -> MemorySnapshot {
    MemorySnapshot {
        total_kb: parse_meminfo_field("MemTotal").unwrap_or(0),
        available_kb: parse_meminfo_field("MemAvailable").unwrap_or(0),
        swap_total_kb: parse_meminfo_field("SwapTotal").unwrap_or(0),
        swap_free_kb: parse_meminfo_field("SwapFree").unwrap_or(0),
    }
}

pub fn read_load() -> LoadSnapshot {
    let content = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut parts = content.split_whitespace();
    LoadSnapshot {
        avg1: parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        avg5: parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        avg15: parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn parse_meminfo_field(field: &str) -> Option<u64> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    content
        .lines()
        .find(|l| l.starts_with(field))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
