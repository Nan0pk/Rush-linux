use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::platform;
use crate::types::*;

pub fn run(window_sec: u64) -> CollectionRecord {
    // Hardware profile — reads that are not time-sensitive
    let hardware = HardwareProfile {
        cpu_model: platform::read_cpu_model(),
        cpu_cores: platform::read_cpu_cores(),
        cpu_vendor: platform::read_cpu_vendor(),
        ram_total_kb: platform::read_ram_total_kb(),
        chassis: platform::read_chassis(),
        battery_design_uwh: platform::read_battery_design_uwh(),
        kernel: platform::read_kernel(),
    };

    // Start-of-window snapshots
    let t0 = Instant::now();
    let psi_cpu_start = platform::read_psi_total_us("cpu");
    let psi_io_start = platform::read_psi_total_us("io");
    let rapl_start = platform::read_rapl_uj();
    let bat_start = platform::read_battery_uwh();

    eprintln!(
        "rush-collect: observing {} seconds (Ctrl-C safe — no writes to system)...",
        window_sec
    );
    thread::sleep(Duration::from_secs(window_sec));

    // End-of-window snapshots
    let elapsed = t0.elapsed();
    let elapsed_us = elapsed.as_micros() as u64;
    let elapsed_sec = elapsed.as_secs_f64();

    let psi_cpu_end = platform::read_psi_total_us("cpu");
    let psi_io_end = platform::read_psi_total_us("io");
    let rapl_end = platform::read_rapl_uj();
    let bat_end = platform::read_battery_uwh();

    // Energy deltas
    let rapl_delta = match (rapl_start, rapl_end) {
        (Some(s), Some(e)) if e >= s => Some(e - s),
        _ => None,
    };
    // Battery discharges: energy_now decreases over time
    let bat_delta = match (bat_start, bat_end) {
        (Some(s), Some(e)) if s >= e => Some(s - e),
        _ => None,
    };

    let avg_watts_rapl = rapl_delta.map(|d| {
        let j = d as f64 * 1e-6; // µJ → J
        round2(j / elapsed_sec)
    });
    let avg_watts_battery = bat_delta.map(|d| {
        let j = d as f64 * 3.6e-3; // µWh → J
        round2(j / elapsed_sec)
    });

    let counter_used = if rapl_delta.is_some() {
        "rapl_sysfs"
    } else if bat_delta.is_some() {
        "battery_sysfs"
    } else {
        "none"
    }
    .to_string();

    // PSI window
    let psi = match (psi_cpu_start, psi_cpu_end, psi_io_start, psi_io_end) {
        (Some(cs), Some(ce), Some(is), Some(ie)) if elapsed_us > 0 => {
            let cpu_stall =
                ce.saturating_sub(cs) as f64 / elapsed_us as f64 * 100.0;
            let io_stall =
                ie.saturating_sub(is) as f64 / elapsed_us as f64 * 100.0;
            Some(PsiWindow {
                cpu_stall_pct: round2(cpu_stall),
                io_stall_pct: round2(io_stall),
                elapsed_us,
            })
        }
        _ => None,
    };

    // Point-in-time snapshots (taken at end of window to reflect load conditions)
    let thermal = platform::read_thermal();
    let freq = platform::read_freq();
    let memory = platform::read_memory();
    let load = platform::read_load();
    let ac_online = platform::read_ac_online();

    CollectionRecord {
        schema_version: SCHEMA_VERSION,
        collected_at: utc_now(),
        os: std::env::consts::OS,
        window_sec,
        hardware,
        energy: EnergyWindow {
            ac_online,
            counter_used,
            rapl_delta_uj: rapl_delta,
            battery_delta_uwh: bat_delta,
            avg_watts_rapl,
            avg_watts_battery,
        },
        psi,
        thermal,
        freq,
        memory,
        load,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Howard Hinnant civil-from-days (no chrono dependency)
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let (h, m, s) = (day_secs / 3600, (day_secs % 3600) / 60, day_secs % 60);

    let z = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mp < 10 { y } else { y + 1 };

    format!("{yr:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}
