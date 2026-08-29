//! The modelled machine physics.
//!
//! Every number below is an *assumption about hardware*, not about optid. The
//! model is handed the machine state that optid actually produced — the values
//! read back out of the simulated control files — plus the offered workload,
//! and it computes latency, throughput, completed work, stall pressure, energy
//! and temperature from that state alone. It has no knowledge of which arm is
//! running, no notion of a desired answer, and no branch on "optid is enabled".
//! A harmful action therefore scores as harmful for the same reason a helpful
//! one scores as helpful: because of the state it left behind.
//!
//! Only `+ - * /` and comparisons are used, so a run is bit-reproducible.
//!
//! Every quantity is modelled. None of it is measured. No statement produced
//! from this module is a claim about real hardware.

use std::collections::BTreeMap;

use serde::Serialize;

/// The modelled environment: everything about the machine's situation that
/// optid does not control.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct EnvState {
    pub(crate) on_ac: bool,
    pub(crate) battery_pct: u8,
    pub(crate) ambient_c: f64,
    pub(crate) die_temp_c: f64,
    pub(crate) skin_temp_c: f64,
    pub(crate) loadavg_1: f64,
    pub(crate) cpu_pressure: f64,
    pub(crate) memory_pressure: f64,
    pub(crate) io_pressure: f64,
    /// Heat from something other than the CPU (charging, dGPU, sunlight).
    pub(crate) external_heat_w: f64,
    /// Whether the active swap device is ZRAM. High swappiness is only a good
    /// trade when it is.
    pub(crate) memory_ratio_zram: bool,
}

/// The offered workload. Demand is what the user asks for; it does not depend
/// on what the machine manages to deliver.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct WorkloadProfile {
    pub(crate) id: String,
    /// Core-equivalents of CPU work demanded per second.
    pub(crate) cpu_demand: f64,
    /// Share of demand that is foreground / latency sensitive.
    pub(crate) foreground_share: f64,
    /// CPU wakeups per second that must be serviced.
    pub(crate) wake_rate_hz: f64,
    /// Storage operations per second.
    pub(crate) io_rate_hz: f64,
    /// Device wakeups per second across attached devices.
    pub(crate) device_wake_rate_hz: f64,
    /// Working set relative to resident memory. Above 1.0 the machine reclaims.
    pub(crate) memory_ratio: f64,
    /// Reference service time of one foreground request, before any wake cost.
    pub(crate) service_us: f64,
}

/// A named set of hardware assumptions. Sensitivity analysis re-evaluates the
/// same recorded machine trajectory under every set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Assumptions {
    pub(crate) id: String,
    pub(crate) description: String,

    pub(crate) reference_capacity: f64,
    pub(crate) epp_perf_performance: f64,
    pub(crate) epp_perf_balance_performance: f64,
    pub(crate) epp_perf_balance_power: f64,
    pub(crate) epp_perf_power: f64,
    pub(crate) epp_power_performance: f64,
    pub(crate) epp_power_balance_performance: f64,
    pub(crate) epp_power_balance_power: f64,
    pub(crate) epp_power_power: f64,
    pub(crate) profile_perf_performance: f64,
    pub(crate) profile_perf_balanced: f64,
    pub(crate) profile_perf_low_power: f64,
    pub(crate) profile_power_performance: f64,
    pub(crate) profile_power_balanced: f64,
    pub(crate) profile_power_low_power: f64,

    pub(crate) cpu_active_w: f64,
    pub(crate) cpu_idle_deep_w: f64,
    pub(crate) cpu_idle_shallow_w: f64,
    pub(crate) cpu_deep_exit_us: f64,
    pub(crate) cpu_shallow_exit_us: f64,
    pub(crate) soc_base_w: f64,

    pub(crate) panel_min_w: f64,
    pub(crate) panel_max_w: f64,

    pub(crate) device_idle_w: f64,
    pub(crate) device_suspended_w: f64,
    pub(crate) device_deep_resume_us: f64,
    pub(crate) device_shallow_resume_us: f64,
    pub(crate) aspm_l1_save_w: f64,
    pub(crate) aspm_l1_exit_us: f64,

    pub(crate) alpm_medium_save_w: f64,
    pub(crate) alpm_medium_exit_us: f64,
    pub(crate) alpm_dipm_save_w: f64,
    pub(crate) alpm_dipm_exit_us: f64,
    pub(crate) alpm_min_save_w: f64,
    pub(crate) alpm_min_exit_us: f64,

    pub(crate) io_service_us: f64,
    pub(crate) io_queue_us_per_unit: f64,
    pub(crate) dirty_reference_bytes: f64,
    /// What `vm.dirty_bytes = 0` actually means: the kernel falls back to
    /// `vm.dirty_ratio` (20% of memory by default). Modelled as a byte figure
    /// so a machine that has never been tuned is not modelled as having a zero
    /// dirty budget.
    pub(crate) dirty_ratio_default_bytes: f64,
    pub(crate) dirty_burst_us: f64,
    pub(crate) swap_reclaim_gain: f64,
    pub(crate) swap_no_zram_penalty: f64,
    pub(crate) memory_stall_gain: f64,

    pub(crate) queue_penalty_us: f64,
    pub(crate) thermal_resistance_c_per_w: f64,
    pub(crate) thermal_tau_s: f64,
    pub(crate) throttle_onset_c: f64,
    pub(crate) throttle_gain_per_c: f64,
    pub(crate) throttle_floor: f64,
    pub(crate) skin_coupling: f64,
    pub(crate) battery_capacity_wh: f64,
}

impl Assumptions {
    pub(crate) fn nominal() -> Self {
        Self {
            id: "nominal".to_string(),
            description: "Mid-range 15 W-class mobile x86 laptop, flash storage, \
                          single internal panel. Central estimates."
                .to_string(),
            reference_capacity: 4.0,
            epp_perf_performance: 1.00,
            epp_perf_balance_performance: 0.97,
            epp_perf_balance_power: 0.90,
            epp_perf_power: 0.80,
            epp_power_performance: 1.00,
            epp_power_balance_performance: 0.92,
            epp_power_balance_power: 0.80,
            epp_power_power: 0.68,
            profile_perf_performance: 1.00,
            profile_perf_balanced: 0.97,
            profile_perf_low_power: 0.88,
            profile_power_performance: 1.00,
            profile_power_balanced: 0.93,
            profile_power_low_power: 0.82,
            cpu_active_w: 22.0,
            cpu_idle_deep_w: 0.60,
            cpu_idle_shallow_w: 3.20,
            cpu_deep_exit_us: 1000.0,
            cpu_shallow_exit_us: 2.0,
            soc_base_w: 2.0,
            panel_min_w: 0.80,
            panel_max_w: 5.50,
            device_idle_w: 0.45,
            device_suspended_w: 0.05,
            device_deep_resume_us: 2500.0,
            device_shallow_resume_us: 25.0,
            aspm_l1_save_w: 0.35,
            aspm_l1_exit_us: 60.0,
            alpm_medium_save_w: 0.25,
            alpm_medium_exit_us: 200.0,
            alpm_dipm_save_w: 0.42,
            alpm_dipm_exit_us: 500.0,
            alpm_min_save_w: 0.65,
            alpm_min_exit_us: 1500.0,
            io_service_us: 120.0,
            io_queue_us_per_unit: 40.0,
            dirty_reference_bytes: 134_217_728.0,
            dirty_ratio_default_bytes: 3_435_973_836.0,
            dirty_burst_us: 420.0,
            swap_reclaim_gain: 0.40,
            swap_no_zram_penalty: 0.30,
            memory_stall_gain: 55.0,
            queue_penalty_us: 2200.0,
            thermal_resistance_c_per_w: 0.55,
            thermal_tau_s: 45.0,
            throttle_onset_c: 92.0,
            throttle_gain_per_c: 0.020,
            throttle_floor: 0.60,
            skin_coupling: 0.35,
            battery_capacity_wh: 52.0,
        }
    }

    /// The full sensitivity grid. Each variant moves one family of assumptions
    /// to a defensible extreme; a result whose sign is not stable across the
    /// whole grid is reported as assumption-sensitive rather than as a finding.
    pub(crate) fn grid() -> Vec<Self> {
        let mut sets = vec![Self::nominal()];

        let mut set = Self::nominal();
        set.id = "epp_effect_weak".to_string();
        set.description =
            "EPP and platform profile barely move performance or power (firmware ignores most of the hint)."
                .to_string();
        set.epp_perf_balance_performance = 0.995;
        set.epp_perf_balance_power = 0.98;
        set.epp_perf_power = 0.96;
        set.epp_power_balance_performance = 0.985;
        set.epp_power_balance_power = 0.96;
        set.epp_power_power = 0.93;
        set.profile_perf_balanced = 0.995;
        set.profile_perf_low_power = 0.97;
        set.profile_power_balanced = 0.985;
        set.profile_power_low_power = 0.96;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "epp_effect_strong".to_string();
        set.description =
            "EPP and platform profile move performance and power strongly (aggressive firmware)."
                .to_string();
        set.epp_perf_balance_performance = 0.93;
        set.epp_perf_balance_power = 0.80;
        set.epp_perf_power = 0.62;
        set.epp_power_balance_performance = 0.84;
        set.epp_power_balance_power = 0.64;
        set.epp_power_power = 0.48;
        set.profile_perf_balanced = 0.93;
        set.profile_perf_low_power = 0.75;
        set.profile_power_balanced = 0.86;
        set.profile_power_low_power = 0.68;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "idle_depth_cheap".to_string();
        set.description = "Deep idle is shallow-exit and barely cheaper than a shallow state, so \
                           a PM QoS floor costs almost no energy."
            .to_string();
        set.cpu_idle_shallow_w = 0.95;
        set.cpu_deep_exit_us = 220.0;
        set.device_deep_resume_us = 600.0;
        set.device_suspended_w = 0.30;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "idle_depth_expensive".to_string();
        set.description = "Deep idle saves a lot and exits slowly, so a PM QoS floor is a large \
                           energy cost and a large latency win."
            .to_string();
        set.cpu_idle_shallow_w = 5.40;
        set.cpu_deep_exit_us = 2400.0;
        set.device_deep_resume_us = 6000.0;
        set.device_suspended_w = 0.01;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "link_pm_weak".to_string();
        set.description =
            "Link power management saves little and costs little (ASPM/ALPM barely engage)."
                .to_string();
        set.aspm_l1_save_w = 0.06;
        set.aspm_l1_exit_us = 12.0;
        set.alpm_medium_save_w = 0.04;
        set.alpm_medium_exit_us = 40.0;
        set.alpm_dipm_save_w = 0.07;
        set.alpm_dipm_exit_us = 90.0;
        set.alpm_min_save_w = 0.11;
        set.alpm_min_exit_us = 260.0;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "link_pm_strong".to_string();
        set.description =
            "Link power management saves a lot and costs a lot of wake latency.".to_string();
        set.aspm_l1_save_w = 0.70;
        set.aspm_l1_exit_us = 180.0;
        set.alpm_medium_save_w = 0.50;
        set.alpm_medium_exit_us = 600.0;
        set.alpm_dipm_save_w = 0.85;
        set.alpm_dipm_exit_us = 1400.0;
        set.alpm_min_save_w = 1.30;
        set.alpm_min_exit_us = 4200.0;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "thermal_tight".to_string();
        set.description =
            "Small chassis: high thermal resistance, fast time constant, early throttle."
                .to_string();
        set.thermal_resistance_c_per_w = 0.95;
        set.thermal_tau_s = 22.0;
        set.throttle_onset_c = 84.0;
        set.throttle_gain_per_c = 0.030;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "thermal_loose".to_string();
        set.description =
            "Large chassis: low thermal resistance, slow time constant, late throttle.".to_string();
        set.thermal_resistance_c_per_w = 0.28;
        set.thermal_tau_s = 90.0;
        set.throttle_onset_c = 97.0;
        set.throttle_gain_per_c = 0.012;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "display_dominant".to_string();
        set.description =
            "Bright, power-hungry panel dominates the platform power budget.".to_string();
        set.panel_min_w = 1.60;
        set.panel_max_w = 11.0;
        sets.push(set);

        let mut set = Self::nominal();
        set.id = "memory_reclaim_weak".to_string();
        set.description =
            "Swappiness barely changes reclaim behaviour; dirty limits barely change writeback."
                .to_string();
        set.swap_reclaim_gain = 0.06;
        set.swap_no_zram_penalty = 0.05;
        set.dirty_burst_us = 60.0;
        sets.push(set);

        sets
    }
}

/// One control cycle's modelled outcome.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct StepMetrics {
    pub(crate) foreground_p99_latency_us: f64,
    pub(crate) foreground_mean_latency_us: f64,
    pub(crate) throughput_ops_per_s: f64,
    pub(crate) completed_work_units: f64,
    pub(crate) cpu_stall_pct: f64,
    pub(crate) memory_stall_pct: f64,
    pub(crate) io_stall_pct: f64,
    pub(crate) mean_power_w: f64,
    pub(crate) energy_j: f64,
    pub(crate) die_temp_end_c: f64,
    pub(crate) iterations: u64,
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// Round to six decimals so the serialised bundle is byte-stable.
fn round6(value: f64) -> f64 {
    let scaled = value * 1_000_000.0;
    let rounded = if scaled >= 0.0 {
        (scaled + 0.5) as i64
    } else {
        (scaled - 0.5) as i64
    };
    rounded as f64 / 1_000_000.0
}

fn get<'a>(active: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    active.get(key).map(|value| value.as_str())
}

fn first_with_prefix<'a>(
    active: &'a BTreeMap<String, String>,
    prefix: &str,
) -> Option<(&'a str, &'a str)> {
    active
        .iter()
        .find(|(key, _)| key.starts_with(prefix))
        .map(|(key, value)| (key.as_str(), value.as_str()))
}

fn epp_factors(assumptions: &Assumptions, epp: &str) -> (f64, f64) {
    match epp {
        "performance" => (
            assumptions.epp_perf_performance,
            assumptions.epp_power_performance,
        ),
        "balance_performance" => (
            assumptions.epp_perf_balance_performance,
            assumptions.epp_power_balance_performance,
        ),
        "balance_power" => (
            assumptions.epp_perf_balance_power,
            assumptions.epp_power_balance_power,
        ),
        "power" => (assumptions.epp_perf_power, assumptions.epp_power_power),
        _ => (
            assumptions.epp_perf_balance_performance,
            assumptions.epp_power_balance_performance,
        ),
    }
}

fn profile_factors(assumptions: &Assumptions, profile: &str) -> (f64, f64) {
    match profile {
        "performance" => (
            assumptions.profile_perf_performance,
            assumptions.profile_power_performance,
        ),
        "low-power" => (
            assumptions.profile_perf_low_power,
            assumptions.profile_power_low_power,
        ),
        _ => (
            assumptions.profile_perf_balanced,
            assumptions.profile_power_balanced,
        ),
    }
}

fn alpm_effect(assumptions: &Assumptions, policy: &str) -> (f64, f64) {
    match policy {
        "medium_power" => (
            assumptions.alpm_medium_save_w,
            assumptions.alpm_medium_exit_us,
        ),
        "med_power_with_dipm" => (assumptions.alpm_dipm_save_w, assumptions.alpm_dipm_exit_us),
        "min_power" => (assumptions.alpm_min_save_w, assumptions.alpm_min_exit_us),
        _ => (0.0, 0.0),
    }
}

/// Evaluate one control cycle from the machine state that is active right now.
pub(crate) fn evaluate_step(
    active: &BTreeMap<String, String>,
    env: &EnvState,
    workload: &WorkloadProfile,
    assumptions: &Assumptions,
    step_seconds: f64,
) -> StepMetrics {
    // ── CPU capability and power ────────────────────────────────────────────
    let epp = first_with_prefix(active, "cpu_epp:")
        .map(|(_, value)| value)
        .unwrap_or("balance_performance");
    let (epp_perf, epp_power) = epp_factors(assumptions, epp);
    let profile = get(active, "platform_profile:acpi").unwrap_or("balanced");
    let (profile_perf, profile_power) = profile_factors(assumptions, profile);

    let over_throttle = env.die_temp_c - assumptions.throttle_onset_c;
    let thermal_derate = if over_throttle > 0.0 {
        clamp(
            1.0 - assumptions.throttle_gain_per_c * over_throttle,
            assumptions.throttle_floor,
            1.0,
        )
    } else {
        1.0
    };

    let capacity = assumptions.reference_capacity * epp_perf * profile_perf * thermal_derate;
    let demand = workload.cpu_demand;
    let served = if demand < capacity { demand } else { capacity };
    let utilisation = if capacity > 0.0 {
        clamp(demand / capacity, 0.0, 1.0)
    } else {
        1.0
    };
    let cpu_stall_pct = if demand > 0.0 && demand > capacity {
        clamp((demand - capacity) / demand, 0.0, 1.0) * 100.0
    } else {
        0.0
    };

    // ── CPU wake latency and idle residency ─────────────────────────────────
    let qos_us = get(active, "cpu_dma_latency:cpu")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(f64::MAX);
    let deep_exit = assumptions.cpu_deep_exit_us;
    let shallow_exit = assumptions.cpu_shallow_exit_us;
    let cpu_wake_us = if qos_us < deep_exit {
        if qos_us < shallow_exit {
            shallow_exit
        } else {
            qos_us
        }
    } else {
        deep_exit
    };
    // Fraction of deep-idle residency the floor gives up.
    let depth_lost = if deep_exit > shallow_exit {
        clamp(
            (deep_exit - cpu_wake_us) / (deep_exit - shallow_exit),
            0.0,
            1.0,
        )
    } else {
        0.0
    };
    let idle_w = assumptions.cpu_idle_deep_w
        + (assumptions.cpu_idle_shallow_w - assumptions.cpu_idle_deep_w) * depth_lost;
    let cpu_power_w = idle_w * (1.0 - utilisation)
        + assumptions.cpu_active_w * epp_power * profile_power * utilisation;

    // ── Devices: runtime PM, per-device PM QoS, PCIe ASPM ───────────────────
    let mut device_power_w = 0.0;
    let mut device_wake_us_total = 0.0;
    let mut device_count = 0.0;
    for (key, value) in active {
        if let Some(id) = key.strip_prefix("runtime_pm_control:") {
            device_count += 1.0;
            let ceiling = active
                .get(&format!("device_resume_latency:{id}"))
                .and_then(|value| value.parse::<f64>().ok());
            let deep_allowed = match ceiling {
                // `0` is the kernel's "no constraint" value on this attribute.
                Some(limit) if limit > 0.0 => limit >= assumptions.device_deep_resume_us,
                _ => true,
            };
            let suspended = value == "auto" && deep_allowed;
            if suspended {
                device_power_w += assumptions.device_suspended_w;
                device_wake_us_total += assumptions.device_deep_resume_us;
            } else {
                device_power_w += assumptions.device_idle_w;
                device_wake_us_total += assumptions.device_shallow_resume_us;
            }
        }
        if let Some(_id) = key.strip_prefix("pci_aspm:") {
            if value == "1" {
                device_power_w -= assumptions.aspm_l1_save_w;
                device_wake_us_total += assumptions.aspm_l1_exit_us;
            }
        }
    }
    if device_power_w < 0.0 {
        device_power_w = 0.0;
    }
    let mean_device_wake_us = if device_count > 0.0 {
        device_wake_us_total / device_count
    } else {
        0.0
    };

    // ── Storage link power management ───────────────────────────────────────
    let mut alpm_save_w = 0.0;
    let mut alpm_exit_us = 0.0;
    for (key, value) in active {
        if key.starts_with("sata_alpm:") {
            let (save, exit) = alpm_effect(assumptions, value);
            alpm_save_w += save;
            alpm_exit_us += exit;
        }
    }

    // ── Display ─────────────────────────────────────────────────────────────
    let mut display_w = assumptions.panel_max_w;
    if let Some((key, value)) = first_with_prefix(active, "backlight:") {
        let _ = key;
        if let Ok(raw) = value.parse::<f64>() {
            // `max_brightness` is fixed by the machine spec at 960.
            let ratio = clamp(raw / 960.0, 0.0, 1.0);
            let curve = 0.5 * ratio + 0.5 * ratio * ratio;
            display_w = assumptions.panel_min_w
                + (assumptions.panel_max_w - assumptions.panel_min_w) * curve;
        }
    }

    // ── Memory and writeback behaviour ──────────────────────────────────────
    let swappiness = get(active, "vm_sysctl:swappiness")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(60.0);
    let dirty_bytes = get(active, "vm_sysctl:dirty_bytes")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| {
            if value <= 0.0 {
                assumptions.dirty_ratio_default_bytes
            } else {
                value
            }
        })
        .unwrap_or(assumptions.dirty_ratio_default_bytes);
    let zram = env.memory_ratio_zram;
    let reclaim_gain = if zram {
        assumptions.swap_reclaim_gain
    } else {
        -assumptions.swap_no_zram_penalty
    };
    let excess_memory = if workload.memory_ratio > 1.0 {
        workload.memory_ratio - 1.0
    } else {
        0.0
    };
    let reclaim_relief = clamp(reclaim_gain * (swappiness / 200.0), -1.0, 1.0);
    let memory_stall_pct = clamp(
        excess_memory * assumptions.memory_stall_gain * (1.0 - reclaim_relief),
        0.0,
        100.0,
    );

    let dirty_ratio = if assumptions.dirty_reference_bytes > 0.0 {
        dirty_bytes / assumptions.dirty_reference_bytes
    } else {
        1.0
    };
    // A larger dirty budget lets more data accumulate before writeback, which
    // lengthens the writeback burst a foreground request can land behind. The
    // clamp bounds the effect: past a few multiples of the reference budget the
    // burst is limited by device bandwidth, not by the dirty limit.
    let writeback_burst_us = assumptions.dirty_burst_us * clamp(dirty_ratio, 0.0, 4.0);

    // ── Latency ─────────────────────────────────────────────────────────────
    let io_share = clamp(workload.io_rate_hz / 4000.0, 0.0, 1.0);
    let io_component_us =
        (assumptions.io_service_us + alpm_exit_us) * io_share + writeback_burst_us * io_share;
    let device_component_us =
        mean_device_wake_us * clamp(workload.device_wake_rate_hz / 200.0, 0.0, 1.0);
    let wake_component_us = cpu_wake_us * clamp(workload.wake_rate_hz / 500.0, 0.0, 1.0);
    // Foreground requests share the machine with background work; the larger
    // the background share, the more queueing a foreground request inherits.
    let contention = 1.0 + (1.0 - clamp(workload.foreground_share, 0.0, 1.0));
    let queue_component_us = assumptions.queue_penalty_us * utilisation * utilisation * contention;
    let memory_component_us = memory_stall_pct * 12.0;

    let mean_latency_us = workload.service_us
        + wake_component_us * 0.35
        + device_component_us * 0.25
        + io_component_us * 0.5
        + queue_component_us * 0.3
        + memory_component_us * 0.4;
    let p99_latency_us = workload.service_us
        + wake_component_us
        + device_component_us
        + io_component_us
        + queue_component_us
        + memory_component_us;

    // ── Throughput and completed work ───────────────────────────────────────
    // Writeback is asynchronous, so the dirty budget mostly moves tail latency
    // rather than steady-state service rate; it contributes a small share of the
    // per-operation cost here.
    let io_capacity_ops =
        1_000_000.0 / (assumptions.io_service_us + alpm_exit_us + writeback_burst_us * 0.05);
    let cpu_bound_ops = served * 1_000_000.0 / (workload.service_us + queue_component_us * 0.25);
    // Combine the CPU and storage limits as serial costs rather than as a hard
    // minimum, so a small parameter change cannot flip the whole result between
    // two regimes.
    let throughput_ops_per_s = if cpu_bound_ops > 0.0 && io_capacity_ops > 0.0 {
        1.0 / (1.0 / cpu_bound_ops + io_share / io_capacity_ops)
    } else {
        0.0
    };
    let stall_scale = 1.0 - clamp(memory_stall_pct / 100.0, 0.0, 0.9);
    let completed_work_units = served * step_seconds * stall_scale;

    // ── Power, energy and temperature ───────────────────────────────────────
    let mut power_w = cpu_power_w + assumptions.soc_base_w + device_power_w + display_w
        - alpm_save_w
        + env.external_heat_w;
    if power_w < 0.2 {
        power_w = 0.2;
    }
    let energy_j = power_w * step_seconds;

    let target_c = env.ambient_c + assumptions.thermal_resistance_c_per_w * power_w;
    let alpha = clamp(step_seconds / assumptions.thermal_tau_s, 0.0, 1.0);
    let die_temp_end_c = env.die_temp_c + alpha * (target_c - env.die_temp_c);

    let io_stall_pct = clamp(
        (io_component_us / (io_component_us + 1000.0)) * 100.0 * io_share,
        0.0,
        100.0,
    );

    StepMetrics {
        foreground_p99_latency_us: round6(p99_latency_us),
        foreground_mean_latency_us: round6(mean_latency_us),
        throughput_ops_per_s: round6(throughput_ops_per_s),
        completed_work_units: round6(completed_work_units),
        cpu_stall_pct: round6(cpu_stall_pct),
        memory_stall_pct: round6(memory_stall_pct),
        io_stall_pct: round6(io_stall_pct),
        mean_power_w: round6(power_w),
        energy_j: round6(energy_j),
        die_temp_end_c: round6(die_temp_end_c),
        iterations: (workload.io_rate_hz.max(1.0) * step_seconds) as u64 + 1,
    }
}

/// Advance the environment using the outcome of the cycle just modelled. This
/// is the feedback path: optid's actions change power, power changes
/// temperature and pressure, and the next snapshot the daemon reads reflects it.
pub(crate) fn advance_env(
    env: &EnvState,
    metrics: &StepMetrics,
    assumptions: &Assumptions,
    step_seconds: f64,
) -> EnvState {
    let die = metrics.die_temp_end_c;
    let skin = env.ambient_c + assumptions.skin_coupling * (die - env.ambient_c);
    let drained_wh = metrics.energy_j / 3600.0;
    let battery_pct = if env.on_ac {
        env.battery_pct
    } else {
        let drop = drained_wh / assumptions.battery_capacity_wh * 100.0;
        let next = env.battery_pct as f64 - drop;
        clamp(next, 0.0, 100.0) as u8
    };
    let _ = step_seconds;
    EnvState {
        on_ac: env.on_ac,
        battery_pct,
        ambient_c: env.ambient_c,
        die_temp_c: round6(die),
        skin_temp_c: round6(skin),
        loadavg_1: env.loadavg_1,
        cpu_pressure: round6(metrics.cpu_stall_pct),
        memory_pressure: round6(metrics.memory_stall_pct),
        io_pressure: round6(metrics.io_stall_pct),
        external_heat_w: env.external_heat_w,
        memory_ratio_zram: env.memory_ratio_zram,
    }
}
