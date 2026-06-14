use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use crate::types::EnergyInfo;
use crate::utils::get_sysfs_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnergySource {
    Battery(PathBuf),
    Rapl(PathBuf),
}

#[derive(Debug, Clone)]
pub struct EnergySample {
    pub time: Instant,
    pub joules: f64,
    pub on_ac: Option<bool>,
}

impl EnergySource {
    pub fn detect() -> Result<Self, String> {
        if let Ok(mocked_type) = env::var("RUSHBENCH_MOCK_ENERGY_SOURCE") {
            match mocked_type.as_str() {
                "battery" => return Ok(EnergySource::Battery(PathBuf::from("/mock/battery"))),
                "rapl" => return Ok(EnergySource::Rapl(PathBuf::from("/mock/rapl"))),
                _ => return Err("no_energy_counter".to_string()),
            }
        }

        let sysfs_root = get_sysfs_root();

        // Prioritize RAPL if it exists and is readable by the current process
        let rapl = sysfs_root.join("sys/class/powercap/intel-rapl:0/energy_uj");
        if rapl.exists() && fs::read_to_string(&rapl).is_ok() {
            return Ok(EnergySource::Rapl(rapl));
        }

        let power_supply = sysfs_root.join("sys/class/power_supply");
        if let Ok(entries) = fs::read_dir(power_supply) {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("BAT") {
                    let path = entry.path().join("energy_now");
                    if path.exists() {
                        return Ok(EnergySource::Battery(path));
                    }
                }
            }
        }

        // Fallback to RAPL even if not readable (will fail on sample, which is expected)
        if rapl.exists() {
            return Ok(EnergySource::Rapl(rapl));
        }

        Err("no_energy_counter".to_string())
    }

    pub fn sample(&self) -> io::Result<EnergySample> {
        let time = Instant::now();
        let on_ac = read_on_ac();

        if let Ok(mocked_joules) = env::var("RUSHBENCH_MOCK_ENERGY_JOULES") {
            let joules: f64 = mocked_joules.parse().unwrap_or(0.0);
            return Ok(EnergySample {
                time,
                joules,
                on_ac,
            });
        }

        match self {
            EnergySource::Battery(path) => {
                let text = fs::read_to_string(path)?;
                let raw_uwh: u64 = text
                    .trim()
                    .parse()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let joules = (raw_uwh as f64) * 0.0036;
                Ok(EnergySample {
                    time,
                    joules,
                    on_ac,
                })
            }
            EnergySource::Rapl(path) => {
                let text = fs::read_to_string(path)?;
                let raw_uj: u64 = text
                    .trim()
                    .parse()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let joules = (raw_uj as f64) * 1e-6;
                Ok(EnergySample {
                    time,
                    joules,
                    on_ac,
                })
            }
        }
    }
}

pub fn calculate_window(
    source: &EnergySource,
    start: &EnergySample,
    end: &EnergySample,
) -> Result<EnergyInfo, String> {
    if start.on_ac != end.on_ac {
        return Err("ac_switch_mid_window".to_string());
    }
    let elapsed = end.time.duration_since(start.time).as_secs_f64();
    if elapsed <= 0.0 {
        return Err("zero_duration_window".to_string());
    }

    let delta_joules = match source {
        EnergySource::Battery(_) => {
            if end.joules > start.joules {
                return Err("counter_wrap".to_string());
            }
            start.joules - end.joules
        }
        EnergySource::Rapl(_) => {
            if end.joules < start.joules {
                return Err("counter_wrap".to_string());
            }
            end.joules - start.joules
        }
    };

    let avg_watts = delta_joules / elapsed;

    let counter_name = match source {
        EnergySource::Battery(path) => {
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy();
            format!("{}/{}", parent, file)
        }
        EnergySource::Rapl(path) => {
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy();
            format!("{}/{}", parent, file)
        }
    };

    Ok(EnergyInfo {
        window_joules: (delta_joules * 100.0).round() / 100.0,
        avg_watts: (avg_watts * 100.0).round() / 100.0,
        counter: counter_name,
    })
}

pub fn read_on_ac() -> Option<bool> {
    if let Ok(ac) = env::var("RUSHBENCH_MOCK_ON_AC") {
        if ac == "null" {
            return None;
        }
        return Some(ac == "true");
    }

    let sysfs_root = get_sysfs_root();
    let entries = fs::read_dir(sysfs_root.join("sys/class/power_supply")).ok()?;
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
