//! T1 — Thermal sensing and pure budget model.
//!
//! Per OPTID-COMPLETION-PLAN.md §4 (T1) and Research Brief 0013, this module
//! owns read-only thermal sensor discovery (hwmon & ACPI thermal_zone), fan
//! RPM reading, and the pure deterministic `ThermalBudget` calculation.
//!
//! No hardware actuation or fan writes are performed by this module.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::kernel_io::KernelRead;
use crate::policy::DomainMode;

/// T1 — Individual thermal sensor reading with normalized units (°C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThermalSensor {
    /// Canonical sensor identity (e.g. `hwmon1:coretemp:Package id 0` or `thermal_zone0:x86_pkg_temp`).
    pub(crate) id: String,
    /// Human-readable label or sensor name.
    pub(crate) label: String,
    /// Current temperature in °C.
    pub(crate) temp_c: f32,
    /// Critical temperature limit in °C if exposed by hardware (`tempM_crit`).
    pub(crate) crit_temp_c: Option<f32>,
    /// `true` if identified as CPU die/package junction sensor.
    pub(crate) is_die: bool,
    /// `true` if identified as skin/chassis sensor.
    pub(crate) is_skin: bool,
}

/// T1 — Read-only fan sensor reading with normalized RPM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FanSensor {
    /// Canonical fan identity (e.g. `hwmon0:fan1` or `acpi:ibm_fan`).
    pub(crate) id: String,
    /// Human-readable fan label.
    pub(crate) label: String,
    /// Current fan speed in RPM (0 if stopped or unreadable).
    pub(crate) rpm: u32,
}

/// T1 — Configuration parameters for the thermal budget engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThermalConfig {
    /// Per-domain runtime mode for thermal sensing (`Off`, `Observe`, `Actuate`).
    pub(crate) mode: DomainMode,
    /// Lower temperature threshold in °C below which system is considered `Cool` (default 60.0°C).
    pub(crate) thermal_lo_c: f32,
    /// Upper temperature threshold in °C above which maximum derating occurs (default 95.0°C).
    pub(crate) thermal_hi_c: f32,
    /// Hysteresis band in °C for lower threshold cooling transitions (default 2.0°C).
    pub(crate) hysteresis_c: f32,
    /// Optional skin temperature limit in °C (default 43.0°C per IEC 62368-1).
    pub(crate) skin_temp_limit_c: Option<f32>,
}

impl Default for ThermalConfig {
    fn default() -> Self {
        Self {
            mode: DomainMode::Observe,
            thermal_lo_c: 60.0,
            thermal_hi_c: 95.0,
            hysteresis_c: 2.0,
            skin_temp_limit_c: Some(43.0),
        }
    }
}

/// T1 — State of thermal budget derating.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThermalBudgetState {
    /// System is cool; full headroom available (derating ratio = 0.0).
    Cool,
    /// Thermal derating active (derating ratio in (0.0, 1.0)).
    Derating,
    /// Thermally constrained; maximum derating applied (derating ratio = 1.0).
    Constrained,
}

/// T1 — The pure, deterministic thermal budget result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThermalBudget {
    /// Categorical budget state.
    pub(crate) state: ThermalBudgetState,
    /// Derating ratio in [0.0, 1.0] where 0.0 = full headroom, 1.0 = max constrained.
    pub(crate) derating_ratio: f32,
    /// Highest sampled CPU die temperature in °C.
    pub(crate) max_die_temp_c: Option<f32>,
    /// Highest sampled skin/chassis temperature in °C.
    pub(crate) skin_temp_c: Option<f32>,
    /// Highest sampled fan speed in RPM.
    pub(crate) max_fan_rpm: Option<u32>,
    /// Explanations and reasons for budget state calculation.
    pub(crate) reasons: Vec<String>,
}

impl Default for ThermalBudget {
    fn default() -> Self {
        Self {
            state: ThermalBudgetState::Cool,
            derating_ratio: 0.0,
            max_die_temp_c: None,
            skin_temp_c: None,
            max_fan_rpm: None,
            reasons: vec!["no thermal sensors available; defaulting to cool budget".to_string()],
        }
    }
}

/// T1 — Pure function to compute the deterministic `ThermalBudget` from sensor readings.
///
/// Implements linear derating:
/// - If `T_die <= T_lo`, `derating_ratio = 0.0` (Cool).
/// - If `T_die >= T_hi`, `derating_ratio = 1.0` (Constrained).
/// - Otherwise, `derating_ratio = (T_die - T_lo) / (T_hi - T_lo)`.
///
/// Hysteresis: When `previous` state was `Derating` or `Constrained`, the effective `T_lo`
/// is reduced by `config.hysteresis_c` (e.g. 58.0°C) to prevent oscillation.
pub(crate) fn compute_thermal_budget(
    config: &ThermalConfig,
    sensors: &[ThermalSensor],
    fans: &[FanSensor],
    previous: Option<&ThermalBudget>,
) -> ThermalBudget {
    if config.mode == DomainMode::Off {
        return ThermalBudget {
            state: ThermalBudgetState::Cool,
            derating_ratio: 0.0,
            max_die_temp_c: None,
            skin_temp_c: None,
            max_fan_rpm: None,
            reasons: vec!["thermal domain mode is off".to_string()],
        };
    }

    let mut reasons = Vec::new();

    // 1. Find max CPU die temperature and highest critical limit
    let die_sensors: Vec<&ThermalSensor> = sensors.iter().filter(|s| s.is_die).collect();
    let max_die_sensor = if !die_sensors.is_empty() {
        die_sensors.into_iter().max_by(|a, b| {
            a.temp_c
                .partial_cmp(&b.temp_c)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    } else {
        sensors.iter().max_by(|a, b| {
            a.temp_c
                .partial_cmp(&b.temp_c)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    };

    let max_die_temp_c = max_die_sensor.map(|s| s.temp_c);
    let hw_crit_temp_c = max_die_sensor.and_then(|s| s.crit_temp_c);

    // Dynamic upper threshold if hw crit temp exists: T_hi = min(config.thermal_hi_c, T_crit - 10°C)
    let effective_hi_c = match hw_crit_temp_c {
        Some(crit) => {
            let clamped_hi = (crit - 10.0).max(config.thermal_lo_c + 5.0);
            if clamped_hi < config.thermal_hi_c {
                reasons.push(format!(
                    "clamped thermal_hi to {:.1}°C based on hardware T_crit {:.1}°C - 10°C",
                    clamped_hi, crit
                ));
                clamped_hi
            } else {
                config.thermal_hi_c
            }
        }
        None => config.thermal_hi_c,
    };

    // 2. Check skin temperature constraint
    let skin_sensors: Vec<&ThermalSensor> = sensors.iter().filter(|s| s.is_skin).collect();
    let max_skin_temp_c = skin_sensors.iter().map(|s| s.temp_c).reduce(f32::max);

    // 3. Collect max fan RPM
    let max_fan_rpm = fans.iter().map(|f| f.rpm).max();

    let die_temp = match max_die_temp_c {
        Some(t) => t,
        None => {
            reasons.push("no valid temperature reading found; fallback to cool state".to_string());
            return ThermalBudget {
                state: ThermalBudgetState::Cool,
                derating_ratio: 0.0,
                max_die_temp_c: None,
                skin_temp_c: max_skin_temp_c,
                max_fan_rpm,
                reasons,
            };
        }
    };

    // 4. Hysteresis adjustment for lower threshold
    let was_derating_or_constrained = previous.is_some_and(|p| p.state != ThermalBudgetState::Cool);
    let effective_lo_c = if was_derating_or_constrained {
        let lo_hys = config.thermal_lo_c - config.hysteresis_c;
        reasons.push(format!(
            "applying hysteresis lower threshold {:.1}°C",
            lo_hys
        ));
        lo_hys
    } else {
        config.thermal_lo_c
    };

    // 5. Compute die derating ratio
    let mut derating_ratio = if die_temp <= effective_lo_c {
        0.0
    } else if die_temp >= effective_hi_c {
        1.0
    } else {
        let range = effective_hi_c - effective_lo_c;
        if range <= 0.001 {
            1.0
        } else {
            (die_temp - effective_lo_c) / range
        }
    };

    // 6. Skin temperature override if limit exceeded
    if let (Some(skin_t), Some(limit)) = (max_skin_temp_c, config.skin_temp_limit_c) {
        if skin_t > limit {
            let skin_derating = ((skin_t - limit) / 5.0).clamp(0.2, 1.0);
            if skin_derating > derating_ratio {
                derating_ratio = skin_derating;
                reasons.push(format!(
                    "skin temp {:.1}°C exceeds limit {:.1}°C; elevated derating ratio to {:.2}",
                    skin_t, limit, derating_ratio
                ));
            }
        }
    }

    let state = if derating_ratio <= 0.0 {
        reasons.push(format!(
            "die temp {:.1}°C <= lo threshold {:.1}°C; state = cool",
            die_temp, effective_lo_c
        ));
        ThermalBudgetState::Cool
    } else if derating_ratio >= 1.0 {
        reasons.push(format!(
            "die temp {:.1}°C >= hi threshold {:.1}°C; state = constrained",
            die_temp, effective_hi_c
        ));
        ThermalBudgetState::Constrained
    } else {
        reasons.push(format!(
            "die temp {:.1}°C within [{:.1}°C, {:.1}°C]; derating ratio = {:.2}",
            die_temp, effective_lo_c, effective_hi_c, derating_ratio
        ));
        ThermalBudgetState::Derating
    };

    ThermalBudget {
        state,
        derating_ratio: (derating_ratio * 100.0).round() / 100.0,
        max_die_temp_c: Some(die_temp),
        skin_temp_c: max_skin_temp_c,
        max_fan_rpm,
        reasons,
    }
}

/// T1 — Discover thermal sensors via `KernelRead` interface (hwmon + thermal_zone).
pub(crate) fn discover_thermal_sensors_with(read: &dyn KernelRead) -> Vec<ThermalSensor> {
    let mut results = Vec::new();

    // 1. Discover hwmon temperature sensors
    if let Ok(entries) = read.read_dir(Path::new("/sys/class/hwmon")) {
        for hwmon_dir in entries {
            let name = read
                .read_to_string(&hwmon_dir.join("name"))
                .unwrap_or_default();
            let name = name.trim();
            if name.is_empty() {
                continue;
            }

            // Iterate over temp*_input files
            if let Ok(hwmon_files) = read.read_dir(&hwmon_dir) {
                for file_path in hwmon_files {
                    let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n,
                        None => continue,
                    };

                    if file_name.starts_with("temp") && file_name.ends_with("_input") {
                        let prefix = &file_name[..file_name.len() - 6]; // e.g. "temp1"
                        let text = match read.read_to_string(&file_path) {
                            Ok(t) => t,
                            Err(_) => continue,
                        };

                        let millic: i64 = match text.trim().parse() {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let temp_c = millic as f32 / 1000.0;

                        // Read label if present
                        let label_path = hwmon_dir.join(format!("{}_label", prefix));
                        let label = read.read_to_string(&label_path).unwrap_or_default();
                        let label = label.trim().to_string();

                        // Read crit temp if present
                        let crit_path = hwmon_dir.join(format!("{}_crit", prefix));
                        let crit_temp_c = read
                            .read_to_string(&crit_path)
                            .ok()
                            .and_then(|t| t.trim().parse::<i64>().ok())
                            .map(|m| m as f32 / 1000.0);

                        let hwmon_id = hwmon_dir
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("hwmon");
                        let id = format!(
                            "{}:{}:{}",
                            hwmon_id,
                            name,
                            if label.is_empty() { prefix } else { &label }
                        );

                        let name_lower = name.to_ascii_lowercase();
                        let label_lower = label.to_ascii_lowercase();

                        let is_die = name_lower.contains("coretemp")
                            || name_lower.contains("k10temp")
                            || label_lower.contains("package id")
                            || label_lower.contains("tdie")
                            || label_lower.contains("cpu die");

                        let is_skin = label_lower.contains("skin")
                            || label_lower.contains("chassis")
                            || label_lower.contains("vrm")
                            || label_lower.contains("ambient");

                        results.push(ThermalSensor {
                            id,
                            label: if label.is_empty() {
                                format!("{} {}", name, prefix)
                            } else {
                                label
                            },
                            temp_c,
                            crit_temp_c,
                            is_die,
                            is_skin,
                        });
                    }
                }
            }
        }
    }

    // 2. Discover ACPI thermal_zone sensors
    if let Ok(entries) = read.read_dir(Path::new("/sys/class/thermal")) {
        for tz_dir in entries {
            let dir_name = match tz_dir.file_name().and_then(|n| n.to_str()) {
                Some(n) if n.starts_with("thermal_zone") => n,
                _ => continue,
            };

            let kind = read
                .read_to_string(&tz_dir.join("type"))
                .unwrap_or_default();
            let kind = kind.trim().to_string();

            let temp_text = match read.read_to_string(&tz_dir.join("temp")) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let millic: i64 = match temp_text.trim().parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let temp_c = millic as f32 / 1000.0;

            let id = format!("{}:{}", dir_name, kind);
            let kind_lower = kind.to_ascii_lowercase();

            let is_die = kind_lower.contains("x86_pkg_temp")
                || kind_lower.contains("cpu-thermal")
                || kind_lower.contains("soc-thermal");

            let is_skin = kind_lower.contains("skin");

            results.push(ThermalSensor {
                id,
                label: kind,
                temp_c,
                crit_temp_c: None,
                is_die,
                is_skin,
            });
        }
    }

    results
}

/// T1 — Discover fan RPM sensors via `KernelRead` interface (hwmon + thinkpad ibm/fan).
pub(crate) fn discover_fan_sensors_with(read: &dyn KernelRead) -> Vec<FanSensor> {
    let mut results = Vec::new();

    // 1. hwmon fan sensors
    if let Ok(entries) = read.read_dir(Path::new("/sys/class/hwmon")) {
        for hwmon_dir in entries {
            let hwmon_id = hwmon_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("hwmon");
            let name = read
                .read_to_string(&hwmon_dir.join("name"))
                .unwrap_or_default();
            let name = name.trim();

            if let Ok(hwmon_files) = read.read_dir(&hwmon_dir) {
                for file_path in hwmon_files {
                    let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n,
                        None => continue,
                    };

                    if file_name.starts_with("fan") && file_name.ends_with("_input") {
                        let text = match read.read_to_string(&file_path) {
                            Ok(t) => t,
                            Err(_) => continue,
                        };

                        let rpm: u32 = text.trim().parse().unwrap_or(0);
                        let prefix = &file_name[..file_name.len() - 6];
                        let id = format!("{}:{}:{}", hwmon_id, name, prefix);

                        results.push(FanSensor {
                            id,
                            label: format!("{} {}", name, prefix),
                            rpm,
                        });
                    }
                }
            }
        }
    }

    // 2. ThinkPad /proc/acpi/ibm/fan fallback
    if let Ok(text) = read.read_to_string(Path::new("/proc/acpi/ibm/fan")) {
        for line in text.lines() {
            if line.starts_with("speed:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(rpm) = parts[1].parse::<u32>() {
                        results.push(FanSensor {
                            id: "acpi:ibm_fan".to_string(),
                            label: "ThinkPad IBM Fan".to_string(),
                            rpm,
                        });
                    }
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_budget_cool_below_lo() {
        let config = ThermalConfig::default();
        let sensors = vec![ThermalSensor {
            id: "hwmon0:coretemp:Package id 0".to_string(),
            label: "Package id 0".to_string(),
            temp_c: 45.0,
            crit_temp_c: Some(100.0),
            is_die: true,
            is_skin: false,
        }];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Cool);
        assert_eq!(budget.derating_ratio, 0.0);
        assert_eq!(budget.max_die_temp_c, Some(45.0));
    }

    #[test]
    fn thermal_budget_constrained_above_hi() {
        let config = ThermalConfig::default();
        let sensors = vec![ThermalSensor {
            id: "hwmon0:coretemp:Package id 0".to_string(),
            label: "Package id 0".to_string(),
            temp_c: 98.0,
            crit_temp_c: Some(100.0),
            is_die: true,
            is_skin: false,
        }];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Constrained);
        assert_eq!(budget.derating_ratio, 1.0);
    }

    #[test]
    fn thermal_budget_linear_derating() {
        let config = ThermalConfig {
            mode: DomainMode::Observe,
            thermal_lo_c: 60.0,
            thermal_hi_c: 90.0,
            hysteresis_c: 2.0,
            skin_temp_limit_c: None,
        };
        let sensors = vec![ThermalSensor {
            id: "hwmon0:coretemp:Package id 0".to_string(),
            label: "Package id 0".to_string(),
            temp_c: 75.0,
            crit_temp_c: None,
            is_die: true,
            is_skin: false,
        }];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Derating);
        assert_eq!(budget.derating_ratio, 0.5);
    }

    #[test]
    fn thermal_budget_monotonicity() {
        let config = ThermalConfig::default();
        let mut prev_ratio = 0.0f32;
        for temp in (60..=95).step_by(5) {
            let sensors = vec![ThermalSensor {
                id: "hwmon0:coretemp:Package id 0".to_string(),
                label: "Package id 0".to_string(),
                temp_c: temp as f32,
                crit_temp_c: None,
                is_die: true,
                is_skin: false,
            }];
            let budget = compute_thermal_budget(&config, &sensors, &[], None);
            assert!(
                budget.derating_ratio >= prev_ratio,
                "Budget derating ratio must be monotonic: temp {}°C ratio {} < prev {}",
                temp,
                budget.derating_ratio,
                prev_ratio
            );
            prev_ratio = budget.derating_ratio;
        }
    }

    #[test]
    fn thermal_budget_hysteresis() {
        let config = ThermalConfig {
            mode: DomainMode::Observe,
            thermal_lo_c: 60.0,
            thermal_hi_c: 90.0,
            hysteresis_c: 2.0,
            skin_temp_limit_c: None,
        };

        let prev = ThermalBudget {
            state: ThermalBudgetState::Derating,
            derating_ratio: 0.5,
            max_die_temp_c: Some(75.0),
            skin_temp_c: None,
            max_fan_rpm: None,
            reasons: vec![],
        };

        // Temp at 59°C: without hysteresis (lo=60), it would be Cool.
        // With hysteresis (effective_lo = 60-2 = 58), 59°C remains Derating.
        let sensors = vec![ThermalSensor {
            id: "hwmon0:coretemp:Package id 0".to_string(),
            label: "Package id 0".to_string(),
            temp_c: 59.0,
            crit_temp_c: None,
            is_die: true,
            is_skin: false,
        }];
        let budget = compute_thermal_budget(&config, &sensors, &[], Some(&prev));
        assert_eq!(budget.state, ThermalBudgetState::Derating);
        assert!(budget.derating_ratio > 0.0);
    }
}
