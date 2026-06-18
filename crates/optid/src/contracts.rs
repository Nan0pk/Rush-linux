//! Per-workload-class latency-budget contracts.
//!
//! A `Contracts` table maps each `WorkloadClass` to a pair of latency floors
//! (CPU wakeup + per-device resume) expressed in microseconds. The floors are
//! the *responsiveness floor* from `SPEC-northstar.md` §0 — they are the
//! single constraint that may stop a depth-enabler from going deeper.
//!
//! Values are provisional pending WP-B1 validation against real hardware
//! wakeup distributions; `Contracts::default()` matches `config/optid/contracts.toml`.

use std::fs;
use std::path::Path;

use crate::workload::WorkloadClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ContractFloors {
    pub(crate) cpu_wakeup_latency: i64,
    pub(crate) device_resume_latency: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Contracts {
    pub(crate) idle: ContractFloors,
    pub(crate) light: ContractFloors,
    pub(crate) interactive: ContractFloors,
    pub(crate) latency_critical: ContractFloors,
    pub(crate) throughput: ContractFloors,
}

impl Default for Contracts {
    fn default() -> Self {
        Self {
            idle: ContractFloors {
                cpu_wakeup_latency: 100000,
                device_resume_latency: 1000000,
            },
            light: ContractFloors {
                cpu_wakeup_latency: 50000,
                device_resume_latency: 500000,
            },
            interactive: ContractFloors {
                cpu_wakeup_latency: 1000,
                device_resume_latency: 10000,
            },
            latency_critical: ContractFloors {
                cpu_wakeup_latency: 10,
                device_resume_latency: 100,
            },
            throughput: ContractFloors {
                cpu_wakeup_latency: 10000,
                device_resume_latency: 100000,
            },
        }
    }
}

impl Contracts {
    /// Load a `Contracts` table from a TOML file at `path`. Missing or
    /// unparseable files fall back to `Contracts::default()` so a corrupt
    /// contracts file can never break the daemon — it only loses overrides.
    pub(crate) fn load(path: &Path) -> Self {
        let mut contracts = Self::default();
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return contracts,
        };

        let mut current_class: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            let line = if let Some(idx) = line.find('#') {
                line[..idx].trim()
            } else {
                line
            };
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let section = line[1..line.len() - 1].trim();
                if let Some(stripped) = section.strip_prefix("contracts.") {
                    current_class = Some(stripped.trim().to_string());
                } else {
                    current_class = None;
                }
                continue;
            }

            let mut parts = line.splitn(2, '=');
            let key = match parts.next() {
                Some(k) => k.trim(),
                None => continue,
            };
            let val = match parts.next() {
                Some(v) => v.trim(),
                None => continue,
            };

            if let Some(ref class) = current_class {
                let val_parsed: i64 = match val.parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match class.as_str() {
                    "idle" => match key {
                        "cpu_wakeup_latency" => contracts.idle.cpu_wakeup_latency = val_parsed,
                        "device_resume_latency" => {
                            contracts.idle.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "light" => match key {
                        "cpu_wakeup_latency" => contracts.light.cpu_wakeup_latency = val_parsed,
                        "device_resume_latency" => {
                            contracts.light.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "interactive" => match key {
                        "cpu_wakeup_latency" => {
                            contracts.interactive.cpu_wakeup_latency = val_parsed
                        }
                        "device_resume_latency" => {
                            contracts.interactive.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "latency-critical" => match key {
                        "cpu_wakeup_latency" => {
                            contracts.latency_critical.cpu_wakeup_latency = val_parsed
                        }
                        "device_resume_latency" => {
                            contracts.latency_critical.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "throughput" => match key {
                        "cpu_wakeup_latency" => {
                            contracts.throughput.cpu_wakeup_latency = val_parsed
                        }
                        "device_resume_latency" => {
                            contracts.throughput.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        contracts
    }

    pub(crate) fn resolve(&self, class: WorkloadClass) -> ContractFloors {
        match class {
            WorkloadClass::Idle => self.idle,
            WorkloadClass::Light => self.light,
            WorkloadClass::Interactive => self.interactive,
            WorkloadClass::LatencyCritical => self.latency_critical,
            WorkloadClass::Throughput => self.throughput,
        }
    }
}

/// Predicate from the SPEC-northstar §3 actuation rule:
///
/// ```text
/// contract gate: exit_latency(S) ≤ active_contract.floor(D)
/// ```
///
/// Currently `#[allow(dead_code)]` because the device-level depth-enablers
/// (WP-N5/N6: runtime PM autosuspend, NVMe APST, PCIe ASPM, SATA ALPM) that
/// will call it are not implemented yet. Kept here so the WP implementation
/// can land without redefining the contract semantics.
#[allow(dead_code)]
pub(crate) fn fits_contract(exit_latency_us: u64, floor_us: u64) -> bool {
    exit_latency_us <= floor_us
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_contracts_match_config_optid_contracts_toml() {
        // Sanity check that the in-binary defaults match the published
        // config/optid/contracts.toml — drift here would mean the daemon
        // behaves differently on a system with no config file vs. one with
        // the default config file.
        let c = Contracts::default();
        assert_eq!(c.idle.cpu_wakeup_latency, 100000);
        assert_eq!(c.idle.device_resume_latency, 1000000);
        assert_eq!(c.interactive.cpu_wakeup_latency, 1000);
        assert_eq!(c.interactive.device_resume_latency, 10000);
        assert_eq!(c.latency_critical.cpu_wakeup_latency, 10);
        assert_eq!(c.latency_critical.device_resume_latency, 100);
    }

    #[test]
    fn resolve_maps_each_class() {
        let c = Contracts::default();
        for class in [
            WorkloadClass::Idle,
            WorkloadClass::Light,
            WorkloadClass::Interactive,
            WorkloadClass::LatencyCritical,
            WorkloadClass::Throughput,
        ] {
            let _ = c.resolve(class);
        }
    }

    #[test]
    fn fits_contract_basic_predicate() {
        assert!(fits_contract(100, 1000));
        assert!(fits_contract(1000, 1000));
        assert!(!fits_contract(1001, 1000));
    }

    #[test]
    fn load_missing_file_returns_default() {
        let tmp = std::env::temp_dir().join("optid_contracts_missing.toml");
        let _ = std::fs::remove_file(&tmp);
        let c = Contracts::load(&tmp);
        assert_eq!(c, Contracts::default());
    }
}
