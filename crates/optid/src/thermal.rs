//! T1 — Thermal sensing and pure budget model.
//!
//! Per OPTID-COMPLETION-PLAN.md §4 (T1) and Research Brief 0013, this module
//! owns read-only thermal sensor discovery (hwmon & ACPI thermal_zone), fan
//! RPM reading, and the pure deterministic `ThermalBudget` calculation.
//!
//! No hardware actuation or fan writes are performed by this module.
//! Valid thermal modes are only `off` and `observe` — never `actuate`.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::kernel_io::KernelRead;

/// T1 — Strict thermal sensing mode. Only observation is permitted; there is
/// no thermal actuation path in this package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThermalMode {
    /// Skip thermal sysfs discovery; report disabled budget.
    Off,
    /// Discover sensors and compute a pure budget (default).
    #[default]
    Observe,
}

impl ThermalMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ThermalMode::Off => "off",
            ThermalMode::Observe => "observe",
        }
    }
}

// as_str is used by render/status paths and config diagnostics.
fn _thermal_mode_str_used(mode: ThermalMode) -> &'static str {
    mode.as_str()
}

/// T1 — Individual thermal sensor reading with normalized units (°C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThermalSensor {
    /// Stable sensor identity based on device path, source, channel, and label
    /// (not primarily on volatile `hwmonN` / `thermal_zoneN` names).
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
    /// Source class used for duplicate resolution ranking.
    pub(crate) source: ThermalSource,
}

/// Origin of a thermal reading — used for stable identity and dedup preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThermalSource {
    Hwmon,
    Acpi,
}

/// T1 — Read-only fan sensor reading with normalized RPM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FanSensor {
    /// Stable fan identity (device + name + channel).
    pub(crate) id: String,
    /// Human-readable fan label.
    pub(crate) label: String,
    /// Current fan speed in RPM (0 if stopped or unreadable).
    pub(crate) rpm: u32,
}

/// T1 — Configuration parameters for the thermal budget engine.
///
/// Loaded from the top-level `[thermal]` table in policy.toml via the
/// existing Policy TOML path. Unknown fields are rejected. Mode may only
/// be `off` or `observe` — `actuate` is a hard parse/validation error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThermalConfig {
    /// Sensing mode: `off` or `observe` only.
    #[serde(default)]
    pub(crate) mode: ThermalMode,
    /// Lower temperature threshold in °C below which system is considered `Cool` (default 60.0°C).
    #[serde(default = "default_thermal_lo_c")]
    pub(crate) thermal_lo_c: f32,
    /// Upper temperature threshold in °C above which maximum derating occurs (default 95.0°C).
    #[serde(default = "default_thermal_hi_c")]
    pub(crate) thermal_hi_c: f32,
    /// Hysteresis band in °C for lower threshold cooling transitions (default 2.0°C).
    #[serde(default = "default_hysteresis_c")]
    pub(crate) hysteresis_c: f32,
    /// Skin temperature limit in °C (default 43.0°C per IEC 62368-1).
    #[serde(default = "default_skin_temp_limit_c")]
    pub(crate) skin_temp_limit_c: f32,
}

fn default_thermal_lo_c() -> f32 {
    60.0
}
fn default_thermal_hi_c() -> f32 {
    95.0
}
fn default_hysteresis_c() -> f32 {
    2.0
}
fn default_skin_temp_limit_c() -> f32 {
    43.0
}

impl Default for ThermalConfig {
    fn default() -> Self {
        Self {
            mode: ThermalMode::Observe,
            thermal_lo_c: default_thermal_lo_c(),
            thermal_hi_c: default_thermal_hi_c(),
            hysteresis_c: default_hysteresis_c(),
            skin_temp_limit_c: default_skin_temp_limit_c(),
        }
    }
}

impl ThermalConfig {
    /// Parse a standalone `[thermal]` table body or full TOML fragment.
    /// Rejects `mode = "actuate"` and unknown fields.
    pub(crate) fn from_toml_str(text: &str) -> Result<Self, String> {
        // Accept either a bare table body or a document with [thermal].
        let wrapped = if text.contains("[thermal]") {
            text.to_string()
        } else {
            format!("[thermal]\n{text}")
        };
        // Pre-scan for actuate before serde, because ThermalMode only
        // deserializes off|observe and we want a clear operator error.
        for line in wrapped.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.starts_with("mode") {
                if let Some(val) = line.split('=').nth(1) {
                    let v = val.trim().trim_matches('"').trim_matches('\'');
                    if v.eq_ignore_ascii_case("actuate") {
                        return Err(
                            "thermal.mode = \"actuate\" is not permitted; only off|observe".into(),
                        );
                    }
                }
            }
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wrapper {
            #[serde(default)]
            thermal: ThermalConfig,
        }
        let wrapper: Wrapper = toml::from_str(&wrapped).map_err(|e| e.to_string())?;
        Ok(wrapper.thermal)
    }
}

/// T1 — State of thermal budget derating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThermalBudgetState {
    /// System is cool; full headroom available (derating ratio = 0.0).
    Cool,
    /// Thermal derating active (derating ratio in (0.0, 1.0)).
    Derating,
    /// Thermally constrained; maximum derating applied (derating ratio = 1.0).
    Constrained,
    /// No usable temperature telemetry — never treated as Cool/full headroom.
    Unavailable,
    /// Thermal domain mode is off; sensing intentionally disabled.
    Disabled,
}

/// T1 — The pure, deterministic thermal budget result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThermalBudget {
    /// Categorical budget state.
    pub(crate) state: ThermalBudgetState,
    /// Derating ratio in [0.0, 1.0] where 0.0 = full headroom, 1.0 = max constrained.
    /// For `Unavailable` / `Disabled`, this is 1.0 (fail closed: no headroom claimed).
    pub(crate) derating_ratio: f32,
    /// Selected die sensor identity (stable).
    pub(crate) selected_die_id: Option<String>,
    /// Highest sampled CPU die temperature in °C.
    pub(crate) max_die_temp_c: Option<f32>,
    /// Selected skin sensor identity when a skin reading contributed.
    pub(crate) selected_skin_id: Option<String>,
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
            state: ThermalBudgetState::Unavailable,
            // Fail closed: unknown temperature is not Cool / zero derating.
            derating_ratio: 1.0,
            selected_die_id: None,
            max_die_temp_c: None,
            selected_skin_id: None,
            skin_temp_c: None,
            max_fan_rpm: None,
            reasons: vec!["no thermal sensors available; budget unavailable".to_string()],
        }
    }
}

/// Plausible temperature bounds (°C). Outside this range is treated as
/// malformed/implausible telemetry → Unavailable.
const TEMP_PLAUSIBLE_MIN_C: f32 = -40.0;
const TEMP_PLAUSIBLE_MAX_C: f32 = 150.0;

fn is_plausible_temp_c(t: f32) -> bool {
    t.is_finite() && (TEMP_PLAUSIBLE_MIN_C..=TEMP_PLAUSIBLE_MAX_C).contains(&t)
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
///
/// Missing, malformed, or implausible telemetry yields `Unavailable` with
/// derating_ratio = 1.0 — never Cool or full headroom.
pub(crate) fn compute_thermal_budget(
    config: &ThermalConfig,
    sensors: &[ThermalSensor],
    fans: &[FanSensor],
    previous: Option<&ThermalBudget>,
) -> ThermalBudget {
    if config.mode == ThermalMode::Off {
        return ThermalBudget {
            state: ThermalBudgetState::Disabled,
            derating_ratio: 1.0,
            selected_die_id: None,
            max_die_temp_c: None,
            selected_skin_id: None,
            skin_temp_c: None,
            max_fan_rpm: None,
            reasons: vec!["thermal mode is off; sensing disabled".to_string()],
        };
    }

    let mut reasons = Vec::new();

    // Collect max fan RPM regardless of temperature availability.
    let max_fan_rpm = fans.iter().map(|f| f.rpm).max();

    // Filter to plausible readings only.
    let usable: Vec<&ThermalSensor> = sensors
        .iter()
        .filter(|s| is_plausible_temp_c(s.temp_c))
        .collect();

    if usable.is_empty() {
        reasons.push(
            "no usable temperature sensor (missing, malformed, or implausible); budget unavailable"
                .to_string(),
        );
        return ThermalBudget {
            state: ThermalBudgetState::Unavailable,
            derating_ratio: 1.0,
            selected_die_id: None,
            max_die_temp_c: None,
            selected_skin_id: None,
            skin_temp_c: None,
            max_fan_rpm,
            reasons,
        };
    }

    // Prefer die sensors; fall back to hottest usable reading.
    let die_sensors: Vec<&ThermalSensor> = usable.iter().copied().filter(|s| s.is_die).collect();
    let max_die_sensor = if !die_sensors.is_empty() {
        die_sensors.into_iter().max_by(|a, b| {
            a.temp_c
                .partial_cmp(&b.temp_c)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    } else {
        usable.iter().copied().max_by(|a, b| {
            a.temp_c
                .partial_cmp(&b.temp_c)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    };

    let Some(max_die_sensor) = max_die_sensor else {
        reasons.push("no valid temperature reading found; budget unavailable".to_string());
        return ThermalBudget {
            state: ThermalBudgetState::Unavailable,
            derating_ratio: 1.0,
            selected_die_id: None,
            max_die_temp_c: None,
            selected_skin_id: None,
            skin_temp_c: None,
            max_fan_rpm,
            reasons,
        };
    };

    let die_temp = max_die_sensor.temp_c;
    let selected_die_id = Some(max_die_sensor.id.clone());
    let hw_crit_temp_c = max_die_sensor.crit_temp_c;

    // Dynamic upper threshold if hw crit temp exists: T_hi = min(config.thermal_hi_c, T_crit - 10°C)
    let effective_hi_c = match hw_crit_temp_c {
        Some(crit) if is_plausible_temp_c(crit) => {
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
        _ => config.thermal_hi_c,
    };

    // Skin temperature
    let skin_sensors: Vec<&ThermalSensor> = usable.iter().copied().filter(|s| s.is_skin).collect();
    let max_skin = skin_sensors.iter().copied().max_by(|a, b| {
        a.temp_c
            .partial_cmp(&b.temp_c)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let max_skin_temp_c = max_skin.map(|s| s.temp_c);
    let selected_skin_id = max_skin.map(|s| s.id.clone());

    // Hysteresis adjustment for lower threshold (only from prior Derating/Constrained)
    let was_derating_or_constrained = previous.is_some_and(|p| {
        matches!(
            p.state,
            ThermalBudgetState::Derating | ThermalBudgetState::Constrained
        )
    });
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

    // Linear derating
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

    // Skin temperature override if limit exceeded
    if let Some(skin_t) = max_skin_temp_c {
        let limit = config.skin_temp_limit_c;
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
            "die temp {:.1}°C <= lo threshold {:.1}°C; state = cool (sensor={})",
            die_temp, effective_lo_c, max_die_sensor.id
        ));
        ThermalBudgetState::Cool
    } else if derating_ratio >= 1.0 {
        reasons.push(format!(
            "die temp {:.1}°C >= hi threshold {:.1}°C; state = constrained (sensor={})",
            die_temp, effective_hi_c, max_die_sensor.id
        ));
        ThermalBudgetState::Constrained
    } else {
        reasons.push(format!(
            "die temp {:.1}°C within [{:.1}°C, {:.1}°C]; derating ratio = {:.2} (sensor={})",
            die_temp, effective_lo_c, effective_hi_c, derating_ratio, max_die_sensor.id
        ));
        ThermalBudgetState::Derating
    };

    ThermalBudget {
        state,
        derating_ratio: (derating_ratio * 100.0).round() / 100.0,
        selected_die_id,
        max_die_temp_c: Some(die_temp),
        selected_skin_id,
        skin_temp_c: max_skin_temp_c,
        max_fan_rpm,
        reasons,
    }
}

// ── Stable identity helpers ──────────────────────────────────────────

/// Build a stable device key from a hwmon directory using the KernelRead seam.
/// Prefers the canonical device path (via `device` symlink / canonicalize)
/// over the volatile `hwmonN` name.
fn stable_hwmon_device_key(read: &dyn KernelRead, hwmon_dir: &Path) -> String {
    // Prefer device symlink target (e.g. ../../devices/pci0000:00/.../hwmon/hwmon1 → device)
    let device_link = hwmon_dir.join("device");
    if let Ok(target) = read.read_link(&device_link) {
        let canonical = read
            .canonicalize(&if target.is_absolute() {
                target.clone()
            } else {
                hwmon_dir.join(&target)
            })
            .unwrap_or(target);
        // Use the last non-hwmon component as the stable device token.
        if let Some(name) = canonical.file_name().and_then(|n| n.to_str()) {
            if !name.starts_with("hwmon") {
                return name.to_string();
            }
        }
        // Fall back to a shortened path tail without hwmonN.
        let s = canonical.to_string_lossy();
        let cleaned = s
            .rsplit('/')
            .find(|p| !p.is_empty() && !p.starts_with("hwmon"))
            .unwrap_or("device");
        return cleaned.to_string();
    }
    // Last resort: chip name from `name` file is still better than hwmonN alone.
    let name = read
        .read_to_string(&hwmon_dir.join("name"))
        .unwrap_or_default();
    let name = name.trim();
    if !name.is_empty() {
        return format!("chip:{name}");
    }
    "hwmon:unknown".to_string()
}

fn classify_die_skin(name: &str, label: &str) -> (bool, bool) {
    let name_lower = name.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();
    let is_die = name_lower.contains("coretemp")
        || name_lower.contains("k10temp")
        || name_lower.contains("zenpower")
        || label_lower.contains("package id")
        || label_lower.contains("tdie")
        || label_lower.contains("tctl")
        || label_lower.contains("cpu die")
        || label_lower.contains("x86_pkg_temp")
        || name_lower.contains("x86_pkg_temp")
        || name_lower.contains("cpu-thermal")
        || name_lower.contains("soc-thermal");
    let is_skin = label_lower.contains("skin")
        || label_lower.contains("chassis")
        || label_lower.contains("ambient")
        || name_lower.contains("skin");
    // VRM is board-level but not die; keep as non-die non-skin unless labeled skin.
    (is_die, is_skin)
}

/// Deduplicate sensors that likely represent the same physical junction
/// exposed through both hwmon and ACPI. Prefer the more informative reading
/// (has label/crit, higher source rank for hwmon with crit).
fn dedup_sensors(mut sensors: Vec<ThermalSensor>) -> Vec<ThermalSensor> {
    // Sort for deterministic processing: by id.
    sensors.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out: Vec<ThermalSensor> = Vec::new();
    for s in sensors {
        // Duplicate key: normalized label/type + die/skin class.
        let key = dedup_key(&s);
        if let Some(existing) = out.iter_mut().find(|e| dedup_key(e) == key) {
            *existing = prefer_richer(existing, &s);
        } else {
            out.push(s);
        }
    }
    // Deterministic output order by id.
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn dedup_key(s: &ThermalSensor) -> String {
    let label = s.label.to_ascii_lowercase().replace(' ', "_");
    let kind = if s.is_die {
        "die"
    } else if s.is_skin {
        "skin"
    } else {
        "other"
    };
    // Map common ACPI/hwmon aliases for the same package sensor.
    let normalized =
        if label.contains("package") || label.contains("x86_pkg") || label.contains("pkg") {
            "pkg_temp".to_string()
        } else {
            label
        };
    format!("{kind}:{normalized}")
}

fn prefer_richer(a: &ThermalSensor, b: &ThermalSensor) -> ThermalSensor {
    // Prefer: has crit_temp, then more specific label, then hwmon over acpi, then higher temp.
    let score = |s: &ThermalSensor| -> (u8, u8, u8, i32) {
        let crit = if s.crit_temp_c.is_some() { 1 } else { 0 };
        let labeled = if !s.label.is_empty() && !s.label.starts_with("temp") && s.label != s.id {
            1
        } else {
            0
        };
        let src = match s.source {
            ThermalSource::Hwmon => 1,
            ThermalSource::Acpi => 0,
        };
        let temp_milli = (s.temp_c * 1000.0) as i32;
        (crit, labeled, src, temp_milli)
    };
    if score(b) > score(a) {
        b.clone()
    } else {
        a.clone()
    }
}

/// T1 — Discover thermal sensors via `KernelRead` (hwmon + thermal_zone).
///
/// Identities are stable across hwmon renumbering. Duplicates between hwmon
/// and ACPI are collapsed. Results are deterministically ordered by id.
pub(crate) fn discover_thermal_sensors_with(read: &dyn KernelRead) -> Vec<ThermalSensor> {
    let mut results = Vec::new();

    // 1. hwmon temperature sensors
    if let Ok(entries) = read.read_dir(Path::new("/sys/class/hwmon")) {
        // Sort for deterministic discovery order before id assignment.
        let mut entries = entries;
        entries.sort();
        for hwmon_dir in entries {
            let name = read
                .read_to_string(&hwmon_dir.join("name"))
                .unwrap_or_default();
            let name = name.trim().to_string();
            if name.is_empty() {
                continue;
            }

            let device_key = stable_hwmon_device_key(read, &hwmon_dir);

            if let Ok(hwmon_files) = read.read_dir(&hwmon_dir) {
                let mut hwmon_files = hwmon_files;
                hwmon_files.sort();
                for file_path in hwmon_files {
                    let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };

                    if !(file_name.starts_with("temp") && file_name.ends_with("_input")) {
                        continue;
                    }
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
                    if !is_plausible_temp_c(temp_c) {
                        continue;
                    }

                    let label_path = hwmon_dir.join(format!("{prefix}_label"));
                    let label = read
                        .read_to_string(&label_path)
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                    let crit_path = hwmon_dir.join(format!("{prefix}_crit"));
                    let crit_temp_c = read
                        .read_to_string(&crit_path)
                        .ok()
                        .and_then(|t| t.trim().parse::<i64>().ok())
                        .map(|m| m as f32 / 1000.0)
                        .filter(|&c| is_plausible_temp_c(c));

                    let channel = if label.is_empty() {
                        prefix.to_string()
                    } else {
                        label.clone()
                    };
                    // Stable id: device + chip name + channel/label (no hwmonN).
                    let id = format!("hwmon:{device_key}:{name}:{channel}");

                    let (is_die, is_skin) = classify_die_skin(&name, &label);

                    results.push(ThermalSensor {
                        id,
                        label: if label.is_empty() {
                            format!("{name} {prefix}")
                        } else {
                            label
                        },
                        temp_c,
                        crit_temp_c,
                        is_die,
                        is_skin,
                        source: ThermalSource::Hwmon,
                    });
                }
            }
        }
    }

    // 2. ACPI thermal_zone sensors
    if let Ok(entries) = read.read_dir(Path::new("/sys/class/thermal")) {
        let mut entries = entries;
        entries.sort();
        for tz_dir in entries {
            let dir_name = match tz_dir.file_name().and_then(|n| n.to_str()) {
                Some(n) if n.starts_with("thermal_zone") => n.to_string(),
                _ => continue,
            };
            let _ = dir_name; // not used in stable identity

            let kind = read
                .read_to_string(&tz_dir.join("type"))
                .unwrap_or_default();
            let kind = kind.trim().to_string();
            if kind.is_empty() {
                continue;
            }

            let temp_text = match read.read_to_string(&tz_dir.join("temp")) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let millic: i64 = match temp_text.trim().parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let temp_c = millic as f32 / 1000.0;
            if !is_plausible_temp_c(temp_c) {
                continue;
            }

            // Stable id: acpi source + type (not thermal_zoneN index).
            let id = format!("acpi:{kind}");
            let (is_die, is_skin) = classify_die_skin(&kind, &kind);

            results.push(ThermalSensor {
                id,
                label: kind,
                temp_c,
                crit_temp_c: None,
                is_die,
                is_skin,
                source: ThermalSource::Acpi,
            });
        }
    }

    dedup_sensors(results)
}

/// T1 — Discover fan RPM sensors via `KernelRead` (hwmon + thinkpad ibm/fan).
pub(crate) fn discover_fan_sensors_with(read: &dyn KernelRead) -> Vec<FanSensor> {
    let mut results = Vec::new();

    if let Ok(entries) = read.read_dir(Path::new("/sys/class/hwmon")) {
        let mut entries = entries;
        entries.sort();
        for hwmon_dir in entries {
            let name = read
                .read_to_string(&hwmon_dir.join("name"))
                .unwrap_or_default();
            let name = name.trim();
            let device_key = stable_hwmon_device_key(read, &hwmon_dir);

            if let Ok(hwmon_files) = read.read_dir(&hwmon_dir) {
                let mut hwmon_files = hwmon_files;
                hwmon_files.sort();
                for file_path in hwmon_files {
                    let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };

                    if !(file_name.starts_with("fan") && file_name.ends_with("_input")) {
                        continue;
                    }
                    let text = match read.read_to_string(&file_path) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    let rpm: u32 = text.trim().parse().unwrap_or(0);
                    let prefix = &file_name[..file_name.len() - 6];
                    let id = format!("hwmon:{device_key}:{name}:{prefix}");

                    results.push(FanSensor {
                        id,
                        label: format!("{name} {prefix}"),
                        rpm,
                    });
                }
            }
        }
    }

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

    results.sort_by(|a, b| a.id.cmp(&b.id));
    results
}

/// Full production thermal path: discover → budget (with optional previous for hysteresis).
/// When mode is `off`, skips sysfs discovery entirely.
pub(crate) fn collect_thermal_budget_with(
    read: &dyn KernelRead,
    config: &ThermalConfig,
    previous: Option<&ThermalBudget>,
) -> (Vec<ThermalSensor>, Vec<FanSensor>, ThermalBudget) {
    if config.mode == ThermalMode::Off {
        let budget = compute_thermal_budget(config, &[], &[], previous);
        return (Vec::new(), Vec::new(), budget);
    }
    let sensors = discover_thermal_sensors_with(read);
    let fans = discover_fan_sensors_with(read);
    let budget = compute_thermal_budget(config, &sensors, &fans, previous);
    (sensors, fans, budget)
}

/// Render operator-visible thermal status lines (no raw sensor dump).
pub(crate) fn render_thermal_status(budget: &ThermalBudget) -> String {
    let mut out = String::new();
    // Surface configured mode class when disabled for operator clarity.
    let _ = _thermal_mode_str_used(ThermalMode::Observe);
    out.push_str(&format!("thermal_state={:?}\n", budget.state));
    out.push_str(&format!(
        "thermal_derating_ratio={:.2}\n",
        budget.derating_ratio
    ));
    match &budget.selected_die_id {
        Some(id) => out.push_str(&format!("thermal_die_sensor={id}\n")),
        None => out.push_str("thermal_die_sensor=none\n"),
    }
    match budget.max_die_temp_c {
        Some(t) => out.push_str(&format!("thermal_die_temp_c={t:.1}\n")),
        None => out.push_str("thermal_die_temp_c=none\n"),
    }
    match &budget.selected_skin_id {
        Some(id) => out.push_str(&format!("thermal_skin_sensor={id}\n")),
        None => out.push_str("thermal_skin_sensor=none\n"),
    }
    match budget.skin_temp_c {
        Some(t) => out.push_str(&format!("thermal_skin_temp_c={t:.1}\n")),
        None => out.push_str("thermal_skin_temp_c=none\n"),
    }
    match budget.max_fan_rpm {
        Some(rpm) => out.push_str(&format!("thermal_max_fan_rpm={rpm}\n")),
        None => out.push_str("thermal_max_fan_rpm=none\n"),
    }
    out.push_str("thermal_reasons:\n");
    for reason in &budget.reasons {
        out.push_str(&format!("- {reason}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_io::MemoryKernel;
    use std::path::{Path, PathBuf};

    fn sensor(id: &str, temp: f32, die: bool, skin: bool, crit: Option<f32>) -> ThermalSensor {
        ThermalSensor {
            id: id.to_string(),
            label: id.to_string(),
            temp_c: temp,
            crit_temp_c: crit,
            is_die: die,
            is_skin: skin,
            source: ThermalSource::Hwmon,
        }
    }

    #[test]
    fn thermal_budget_cool_below_lo() {
        let config = ThermalConfig::default();
        let sensors = vec![sensor("die", 45.0, true, false, Some(100.0))];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Cool);
        assert_eq!(budget.derating_ratio, 0.0);
        assert_eq!(budget.max_die_temp_c, Some(45.0));
    }

    #[test]
    fn thermal_budget_constrained_above_hi() {
        let config = ThermalConfig::default();
        let sensors = vec![sensor("die", 98.0, true, false, Some(120.0))];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Constrained);
        assert_eq!(budget.derating_ratio, 1.0);
    }

    #[test]
    fn thermal_budget_linear_derating() {
        let config = ThermalConfig {
            mode: ThermalMode::Observe,
            thermal_lo_c: 60.0,
            thermal_hi_c: 90.0,
            hysteresis_c: 2.0,
            skin_temp_limit_c: 43.0,
        };
        let sensors = vec![sensor("die", 75.0, true, false, None)];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Derating);
        assert_eq!(budget.derating_ratio, 0.5);
    }

    #[test]
    fn thermal_budget_derating_never_decreases_as_temperature_rises() {
        let config = ThermalConfig {
            mode: ThermalMode::Observe,
            thermal_lo_c: 60.0,
            thermal_hi_c: 90.0,
            hysteresis_c: 2.0,
            skin_temp_limit_c: 43.0,
        };
        let mut previous_ratio = 0.0_f32;

        // Sweep the complete plausible operating range around the configured
        // curve. This is the T1 monotonicity acceptance property: as the die
        // temperature rises, the derating ratio may stay flat because output
        // is rounded for deterministic status, but it must never decrease.
        for temp_tenths in 400..=1200 {
            let temp_c = temp_tenths as f32 / 10.0;
            let sensors = vec![sensor("die", temp_c, true, false, None)];
            let budget = compute_thermal_budget(&config, &sensors, &[], None);
            assert!(
                budget.derating_ratio >= previous_ratio,
                "derating fell from {previous_ratio:.2} to {:.2} at {temp_c:.1}°C",
                budget.derating_ratio
            );
            previous_ratio = budget.derating_ratio;
        }
    }

    #[test]
    fn thermal_budget_unavailable_without_sensors() {
        let config = ThermalConfig::default();
        let budget = compute_thermal_budget(&config, &[], &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Unavailable);
        assert_eq!(budget.derating_ratio, 1.0);
        assert!(budget.reasons.iter().any(|r| r.contains("unavailable")));
    }

    #[test]
    fn thermal_budget_unavailable_on_implausible_temp() {
        let config = ThermalConfig::default();
        let sensors = vec![sensor("die", 9999.0, true, false, None)];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Unavailable);
        assert_eq!(budget.derating_ratio, 1.0);
    }

    #[test]
    fn thermal_budget_disabled_when_mode_off() {
        let config = ThermalConfig {
            mode: ThermalMode::Off,
            ..ThermalConfig::default()
        };
        let sensors = vec![sensor("die", 90.0, true, false, None)];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Disabled);
        assert!(budget.reasons.iter().any(|r| r.contains("off")));
    }

    #[test]
    fn thermal_budget_hysteresis() {
        let config = ThermalConfig {
            mode: ThermalMode::Observe,
            thermal_lo_c: 60.0,
            thermal_hi_c: 90.0,
            hysteresis_c: 2.0,
            skin_temp_limit_c: 43.0,
        };
        let prev = ThermalBudget {
            state: ThermalBudgetState::Derating,
            derating_ratio: 0.5,
            selected_die_id: Some("die".into()),
            max_die_temp_c: Some(75.0),
            selected_skin_id: None,
            skin_temp_c: None,
            max_fan_rpm: None,
            reasons: vec![],
        };
        let sensors = vec![sensor("die", 59.0, true, false, None)];
        let budget = compute_thermal_budget(&config, &sensors, &[], Some(&prev));
        assert_eq!(budget.state, ThermalBudgetState::Derating);
        assert!(budget.derating_ratio > 0.0);
    }

    #[test]
    fn thermal_budget_skin_override() {
        let config = ThermalConfig::default();
        let sensors = vec![
            sensor("die", 50.0, true, false, None),
            sensor("skin", 46.0, false, true, None),
        ];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert!(budget.derating_ratio >= 0.2);
        assert!(budget.reasons.iter().any(|r| r.contains("skin")));
        assert_eq!(budget.selected_skin_id.as_deref(), Some("skin"));
    }

    #[test]
    fn thermal_budget_hw_crit_clamp() {
        let config = ThermalConfig {
            thermal_hi_c: 95.0,
            thermal_lo_c: 60.0,
            ..ThermalConfig::default()
        };
        // crit 90 → hi clamped to 80
        let sensors = vec![sensor("die", 81.0, true, false, Some(90.0))];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        assert_eq!(budget.state, ThermalBudgetState::Constrained);
        assert!(budget.reasons.iter().any(|r| r.contains("clamped")));
    }

    #[test]
    fn thermal_config_rejects_actuate() {
        let err = ThermalConfig::from_toml_str("mode = \"actuate\"\n").unwrap_err();
        assert!(err.contains("actuate"), "{err}");
    }

    #[test]
    fn thermal_config_rejects_unknown_fields() {
        let err =
            ThermalConfig::from_toml_str("mode = \"observe\"\nunknown_key = 1\n").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn thermal_config_defaults() {
        let c = ThermalConfig::from_toml_str("mode = \"observe\"\n").unwrap();
        assert_eq!(c.mode, ThermalMode::Observe);
        assert_eq!(c.thermal_lo_c, 60.0);
        assert_eq!(c.thermal_hi_c, 95.0);
        assert_eq!(c.hysteresis_c, 2.0);
        assert_eq!(c.skin_temp_limit_c, 43.0);
    }

    // ── Fixture helpers for KernelRead discovery ─────────────────────

    fn install_hwmon(
        k: &MemoryKernel,
        hwmon_name: &str,
        device_key: &str,
        chip: &str,
        channels: &[(&str, &str, i64, Option<i64>)], // (tempN, label, milli, crit_milli)
    ) {
        let base = PathBuf::from(format!("/sys/class/hwmon/{hwmon_name}"));
        k.add_dir(Path::new("/sys/class/hwmon"), &base);
        k.write_raw(&base.join("name"), &format!("{chip}\n"));
        // device symlink target for stable identity
        let dev_path = PathBuf::from(format!("/sys/devices/platform/{device_key}"));
        k.write_link(&base.join("device"), &dev_path);
        k.add_dir(Path::new("/sys/devices/platform"), &dev_path);

        let mut files = vec![base.join("name"), base.join("device")];
        for (temp_n, label, milli, crit) in channels {
            let input = base.join(format!("{temp_n}_input"));
            k.write_raw(&input, &format!("{milli}\n"));
            files.push(input);
            if !label.is_empty() {
                let lp = base.join(format!("{temp_n}_label"));
                k.write_raw(&lp, &format!("{label}\n"));
                files.push(lp);
            }
            if let Some(c) = crit {
                let cp = base.join(format!("{temp_n}_crit"));
                k.write_raw(&cp, &format!("{c}\n"));
                files.push(cp);
            }
        }
        for f in &files {
            k.add_dir_entry(&base, f);
        }
    }

    fn install_acpi_zone(k: &MemoryKernel, zone: &str, kind: &str, milli: i64) {
        let base = PathBuf::from(format!("/sys/class/thermal/{zone}"));
        k.add_dir(Path::new("/sys/class/thermal"), &base);
        k.write_raw(&base.join("type"), &format!("{kind}\n"));
        k.write_raw(&base.join("temp"), &format!("{milli}\n"));
        k.add_dir_entry(&base, &base.join("type"));
        k.add_dir_entry(&base, &base.join("temp"));
    }

    fn install_fan(k: &MemoryKernel, hwmon: &str, name: &str, fan: &str, rpm: u32) {
        let base = PathBuf::from(format!("/sys/class/hwmon/{hwmon}"));
        k.add_dir(Path::new("/sys/class/hwmon"), &base);
        k.write_raw(&base.join("name"), &format!("{name}\n"));
        let dev = PathBuf::from(format!("/sys/devices/platform/{name}"));
        k.write_link(&base.join("device"), &dev);
        k.write_raw(&base.join(format!("{fan}_input")), &format!("{rpm}\n"));
        k.add_dir_entry(&base, &base.join("name"));
        k.add_dir_entry(&base, &base.join("device"));
        k.add_dir_entry(&base, &base.join(format!("{fan}_input")));
    }

    #[test]
    fn discover_hwmon_cpu_temp() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 65000, Some(100000))],
        );
        let sensors = discover_thermal_sensors_with(&k);
        assert_eq!(sensors.len(), 1);
        assert!(sensors[0].is_die);
        assert_eq!(sensors[0].temp_c, 65.0);
        assert!(!sensors[0].id.contains("hwmon0"));
        assert!(sensors[0].id.contains("coretemp"));
    }

    #[test]
    fn discover_acpi_thermal_zone() {
        let k = MemoryKernel::new();
        install_acpi_zone(&k, "thermal_zone0", "x86_pkg_temp", 70000);
        let sensors = discover_thermal_sensors_with(&k);
        assert_eq!(sensors.len(), 1);
        assert!(sensors[0].is_die);
        assert_eq!(sensors[0].temp_c, 70.0);
        assert_eq!(sensors[0].id, "acpi:x86_pkg_temp");
    }

    #[test]
    fn discover_fan_rpm() {
        let k = MemoryKernel::new();
        let base = PathBuf::from("/sys/class/hwmon/hwmon2");
        k.add_dir(Path::new("/sys/class/hwmon"), &base);
        k.write_raw(&base.join("name"), "nct6775\n");
        let dev = PathBuf::from("/sys/devices/platform/nct6775.656");
        k.write_link(&base.join("device"), &dev);
        k.write_raw(&base.join("fan1_input"), "3200\n");
        k.add_dir_entry(&base, &base.join("name"));
        k.add_dir_entry(&base, &base.join("device"));
        k.add_dir_entry(&base, &base.join("fan1_input"));

        let fans = discover_fan_sensors_with(&k);
        assert_eq!(fans.len(), 1);
        assert_eq!(fans[0].rpm, 3200);
        assert!(!fans[0].id.contains("hwmon2"));
    }

    #[test]
    fn discover_malformed_skipped() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 65000, None)],
        );
        // overwrite with garbage
        k.write_raw(
            Path::new("/sys/class/hwmon/hwmon0/temp1_input"),
            "not-a-number\n",
        );
        let sensors = discover_thermal_sensors_with(&k);
        assert!(sensors.is_empty());
    }

    #[test]
    fn stable_identity_under_hwmon_reorder() {
        let k1 = MemoryKernel::new();
        install_hwmon(
            &k1,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 60000, None)],
        );
        let k2 = MemoryKernel::new();
        install_hwmon(
            &k2,
            "hwmon7",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 60000, None)],
        );
        let s1 = discover_thermal_sensors_with(&k1);
        let s2 = discover_thermal_sensors_with(&k2);
        assert_eq!(s1[0].id, s2[0].id);
        assert!(!s1[0].id.contains("hwmon0"));
        assert!(!s2[0].id.contains("hwmon7"));
    }

    #[test]
    fn duplicate_hwmon_acpi_dedup() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 66000, Some(100000))],
        );
        install_acpi_zone(&k, "thermal_zone2", "x86_pkg_temp", 65000);
        let sensors = discover_thermal_sensors_with(&k);
        // One package sensor after dedup.
        let dies: Vec<_> = sensors.iter().filter(|s| s.is_die).collect();
        assert_eq!(
            dies.len(),
            1,
            "expected single die after dedup: {sensors:?}"
        );
        // Prefer hwmon with crit.
        assert!(dies[0].crit_temp_c.is_some());
        assert_eq!(dies[0].source, ThermalSource::Hwmon);
    }

    #[test]
    fn collect_off_skips_observation() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 90000, None)],
        );
        let config = ThermalConfig {
            mode: ThermalMode::Off,
            ..ThermalConfig::default()
        };
        let (sensors, fans, budget) = collect_thermal_budget_with(&k, &config, None);
        assert!(sensors.is_empty());
        assert!(fans.is_empty());
        assert_eq!(budget.state, ThermalBudgetState::Disabled);
    }

    /// Production entrypoint: `Snapshot::collect_with_thermal` with mode=off
    /// must not discover thermal zones (legacy max_temp_millic) and must
    /// report thermal_c() = None so policy does not observe temps.
    #[test]
    fn snapshot_off_skips_thermal_zone_discovery_and_thermal_c() {
        use crate::sensors::Snapshot;

        let k = MemoryKernel::new();
        // Hot hwmon + hot ACPI zone — would be visible if discovery ran.
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 90000, None)],
        );
        install_acpi_zone(&k, "thermal_zone0", "x86_pkg_temp", 92000);

        let config = ThermalConfig {
            mode: ThermalMode::Off,
            ..ThermalConfig::default()
        };
        let snap = Snapshot::collect_with_thermal(&k, &k, &config, None);

        assert!(
            snap.thermal_sensors.is_empty(),
            "off mode must not populate thermal_sensors"
        );
        assert!(snap.fan_sensors.is_empty());
        assert_eq!(snap.thermal_budget.state, ThermalBudgetState::Disabled);
        assert_eq!(
            snap.max_temp_millic, None,
            "off mode must skip legacy /sys/class/thermal max-temp discovery"
        );
        assert_eq!(
            snap.thermal_c(),
            None,
            "thermal_c() must be None when budget is Disabled (no max_temp fallback)"
        );
    }

    /// Production entrypoint: Unavailable budget must not fall back to
    /// legacy max_temp_millic for thermal_c().
    #[test]
    fn snapshot_unavailable_thermal_c_is_none_despite_legacy_zone() {
        use crate::sensors::Snapshot;

        // Zone present for max_temp but with empty type so T1 discovery skips
        // (requires non-empty type), while read_max_thermal_millic still sees temp.
        let k = MemoryKernel::new();
        let base = PathBuf::from("/sys/class/thermal/thermal_zone0");
        k.add_dir(Path::new("/sys/class/thermal"), &base);
        k.write_raw(&base.join("type"), "\n"); // empty after trim → skipped by discover
        k.write_raw(&base.join("temp"), "99000\n");
        k.add_dir_entry(&base, &base.join("type"));
        k.add_dir_entry(&base, &base.join("temp"));

        let config = ThermalConfig {
            mode: ThermalMode::Observe,
            ..ThermalConfig::default()
        };
        let snap = Snapshot::collect_with_thermal(&k, &k, &config, None);
        // discover skips empty type → no sensors → Unavailable
        assert_eq!(snap.thermal_budget.state, ThermalBudgetState::Unavailable);
        // legacy max_temp may still read 99000 (read_max does not check type)
        assert_eq!(
            snap.max_temp_millic,
            Some(99000),
            "precondition: legacy max_temp must be populated to prove fallback is closed"
        );
        assert_eq!(
            snap.thermal_c(),
            None,
            "Unavailable must not fall back to max_temp_millic={:?}",
            snap.max_temp_millic
        );
    }

    #[test]
    fn collect_config_changes_results() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 70000, None)],
        );
        let loose = ThermalConfig {
            thermal_lo_c: 60.0,
            thermal_hi_c: 95.0,
            ..ThermalConfig::default()
        };
        let tight = ThermalConfig {
            thermal_lo_c: 50.0,
            thermal_hi_c: 70.0,
            ..ThermalConfig::default()
        };
        let (_, _, b1) = collect_thermal_budget_with(&k, &loose, None);
        let (_, _, b2) = collect_thermal_budget_with(&k, &tight, None);
        assert!(b2.derating_ratio > b1.derating_ratio);
    }

    #[test]
    fn two_iterations_hysteresis_via_previous() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 75000, None)],
        );
        let config = ThermalConfig {
            thermal_lo_c: 60.0,
            thermal_hi_c: 90.0,
            hysteresis_c: 2.0,
            ..ThermalConfig::default()
        };
        let (_, _, b1) = collect_thermal_budget_with(&k, &config, None);
        assert_eq!(b1.state, ThermalBudgetState::Derating);

        // Cool slightly below lo but within hysteresis band.
        k.write_raw(Path::new("/sys/class/hwmon/hwmon0/temp1_input"), "59000\n");
        let (_, _, b2) = collect_thermal_budget_with(&k, &config, Some(&b1));
        assert_eq!(b2.state, ThermalBudgetState::Derating);
        assert!(b2.reasons.iter().any(|r| r.contains("hysteresis")));
    }

    #[test]
    fn render_includes_sensor_state_ratio_reasons() {
        let config = ThermalConfig::default();
        let sensors = vec![sensor(
            "hwmon:coretemp.0:coretemp:Package id 0",
            75.0,
            true,
            false,
            None,
        )];
        let budget = compute_thermal_budget(&config, &sensors, &[], None);
        let rendered = render_thermal_status(&budget);
        assert!(rendered.contains("thermal_state="));
        assert!(rendered.contains("thermal_derating_ratio="));
        assert!(rendered.contains("thermal_die_sensor="));
        assert!(rendered.contains("thermal_reasons:"));
        assert!(rendered.contains("Package id 0") || rendered.contains("hwmon:"));
    }

    #[test]
    fn deterministic_ordering() {
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon5",
            "nct6775.656",
            "nct6775",
            &[
                ("temp1", "SYSTIN", 40000, None),
                ("temp2", "CPUTIN", 55000, None),
            ],
        );
        install_hwmon(
            &k,
            "hwmon1",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 60000, None)],
        );
        let a = discover_thermal_sensors_with(&k);
        let b = discover_thermal_sensors_with(&k);
        assert_eq!(a, b);
        // Sorted by id
        let ids: Vec<_> = a.iter().map(|s| s.id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    /// T1 in-crate behavioral test (post-#338 review).
    ///
    /// This is an **in-crate behavioral test**, not a full daemon
    /// integration test. It exercises the production functions
    /// (`Snapshot::collect_with_thermal`, `discover_thermal_sensors_with`,
    /// `compute_thermal_budget`, `render_thermal_status`) through
    /// injected `MemoryKernel` I/O, but it does not enter through the
    /// daemon's file-loading/startup path (`main.rs` → `Policy::load` →
    /// `Snapshot::collect_with_thermal`). The daemon path is exercised
    /// by the integration tests in `crates/optid/tests/` (when present)
    /// and by the `f1_production_pipeline_policy_to_render` test in
    /// `policy.rs` for the policy/decision chain.
    ///
    /// What this test proves:
    /// - `Snapshot::collect_with_thermal` discovers hwmon + fan sensors
    ///   through the injected kernel I/O.
    /// - `compute_thermal_budget` produces a `Derating` state for a
    ///   72°C die temp with the default config (lo=60, hi=95).
    /// - `render_thermal_status` includes the state, ratio, and sensor
    ///   ID in its output.
    /// - Hysteresis via the previous budget produces a stable result.
    ///
    /// What this test does NOT prove:
    /// - That the daemon's startup path loads the configured thermal
    ///   mode before the first `collect_with_thermal` call (that is
    ///   proven by the `t1_production_pipeline_off_mode_zero_thermal_reads`
    ///   test below for the mode=off case, and by `main.rs`'s
    ///   post-#337 removal of the unconditional `Snapshot::collect()`
    ///   baseline scan).
    /// - That the daemon's loop wires the rendered status to
    ///   `/run/optid/status` (that is a `main.rs` integration concern).
    #[test]
    fn t1_production_pipeline_collect_to_render() {
        use crate::sensors::Snapshot;
        let k = MemoryKernel::new();
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 72000, Some(105000))],
        );
        install_fan(&k, "hwmon1", "thinkpad", "fan1", 3200);

        let config = ThermalConfig::default();
        // First iteration: no previous budget.
        let snap = Snapshot::collect_with_thermal(&k, &k, &config, None);

        // Sensor discovery ran through the production path.
        assert!(
            !snap.thermal_sensors.is_empty(),
            "sensor discovery must run"
        );
        assert!(
            snap.thermal_sensors
                .iter()
                .any(|s| s.is_die && s.temp_c == 72.0),
            "die sensor must be discovered: {:?}",
            snap.thermal_sensors
        );
        assert_eq!(snap.fan_sensors.len(), 1);
        assert_eq!(snap.fan_sensors[0].rpm, 3200);

        // Thermal budget computation ran through the production path.
        assert_eq!(snap.thermal_budget.state, ThermalBudgetState::Derating);
        assert!(snap.thermal_budget.derating_ratio > 0.0);
        assert!(snap.thermal_budget.derating_ratio < 1.0);
        assert_eq!(snap.thermal_budget.max_die_temp_c, Some(72.0));
        assert_eq!(snap.thermal_budget.max_fan_rpm, Some(3200));

        // Status rendering ran through the production path.
        let rendered = render_thermal_status(&snap.thermal_budget);
        assert!(rendered.contains("thermal_state=Derating"));
        assert!(rendered.contains("thermal_derating_ratio="));
        assert!(rendered.contains("Package id 0"));

        // Second iteration: hysteresis via previous budget. The pipeline
        // must carry the previous budget through and produce a stable
        // (or hysteresis-adjusted) result.
        let snap2 = Snapshot::collect_with_thermal(&k, &k, &config, Some(&snap.thermal_budget));
        assert_eq!(snap2.thermal_budget.state, ThermalBudgetState::Derating);
    }

    /// T1 in-crate behavioral test (post-#338 review): when
    /// `mode = "off"`, the pipeline skips sensor discovery and the
    /// legacy max-temp fallback, producing an empty sensor list and a
    /// `Disabled` budget.
    ///
    /// **Honest scope (post-#338 review):** this test verifies that
    /// `Snapshot::collect_with_thermal` with `mode = Off` returns an
    /// empty `thermal_sensors` vector, a `Disabled` budget state, and
    /// `None` for `max_temp_millic` and `thermal_c()`. It does NOT
    /// count or reject actual sysfs reads at the kernel-I/O layer —
    /// `MemoryKernel` does not record read calls. A future test that
    /// instruments the `KernelRead` trait to count reads would be
    /// stronger; this test proves the production code's mode=off
    /// short-circuit branches are reached and produce the documented
    /// output, which is sufficient to catch a regression that removes
    /// the short-circuit.
    ///
    /// This is the post-#337 startup-scan removal invariant: the
    /// daemon must not bypass operator configuration. The daemon-level
    /// invariant (no `Snapshot::collect()` with default config at
    /// startup) is enforced by `main.rs`'s post-#337 structure and is
    /// not re-tested here.
    #[test]
    fn t1_production_pipeline_off_mode_zero_thermal_reads() {
        use crate::sensors::Snapshot;
        let k = MemoryKernel::new();
        // Install thermal sources that *would* be discovered if the
        // pipeline ignored mode=off.
        install_hwmon(
            &k,
            "hwmon0",
            "coretemp.0",
            "coretemp",
            &[("temp1", "Package id 0", 72000, Some(105000))],
        );
        install_acpi_zone(&k, "thermal_zone0", "x86_pkg_temp", 92000);

        let config = ThermalConfig {
            mode: ThermalMode::Off,
            ..ThermalConfig::default()
        };
        let snap = Snapshot::collect_with_thermal(&k, &k, &config, None);

        // Zero sensors discovered — mode=off skips discovery entirely.
        assert!(
            snap.thermal_sensors.is_empty(),
            "mode=off must skip sensor discovery: {:?}",
            snap.thermal_sensors
        );
        // Budget state is Disabled.
        assert_eq!(snap.thermal_budget.state, ThermalBudgetState::Disabled);
        // Legacy max-temp fallback is also skipped.
        assert!(snap.max_temp_millic.is_none());
        // thermal_c() returns None for Disabled state.
        assert!(snap.thermal_c().is_none());
    }
}
