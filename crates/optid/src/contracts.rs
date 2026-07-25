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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
                // 1 ms / 1 ms: the previous 10 µs / 100 µs floors were
                // unachievable on non-RT kernels (see tools/external-data/
                // analysis/baselines.json — 0% of OSADL RT-kernel systems
                // reach max cyclictest < 100 µs). 1 ms keeps the floor
                // meaningful for audio/video/game workloads without lying
                // about what the kernel can deliver. The SPEC §1 "floor,
                // never a target to exceed" semantics are preserved.
                cpu_wakeup_latency: 1000,
                device_resume_latency: 1000,
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
            // v0.6 Phase C2: VmGuest uses the interactive contract.
            // PM QoS to the host CPU doesn't propagate across the
            // hypervisor boundary, so the latency-critical floor (now 1 ms)
            // is unenforceable in a guest. VmGuest is a derived execution
            // environment, not a sixth primary class — it resolves to the
            // closest enforceable primary contract (interactive).
            WorkloadClass::VmGuest => self.interactive,
        }
    }
}

/// Predicate from the SPEC-northstar §3 actuation rule:
///
/// ```text
/// contract gate: exit_latency(S) ≤ active_contract.floor(D)
/// ```
///
/// Called by `Actuator::contract_permits` before any depth-enabler write
/// that trades resume latency for power: per-device PM QoS resume latency
/// and runtime-PM autosuspend. A state whose exit latency exceeds the
/// active class's floor is refused.
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
        // the default config file. The exact expected values are pinned
        // here AND in `load_published_contracts_toml_matches_default`
        // below so a fixture drift cannot silently pass.
        let c = Contracts::default();
        assert_eq!(c.idle.cpu_wakeup_latency, 100000);
        assert_eq!(c.idle.device_resume_latency, 1000000);
        assert_eq!(c.interactive.cpu_wakeup_latency, 1000);
        assert_eq!(c.interactive.device_resume_latency, 10000);
        // latency-critical: 1 ms / 1 ms (1000/1000 µs). Corrected from the
        // previous 10/100 µs floors which were unachievable on non-RT
        // kernels and produced permanent budget violations.
        assert_eq!(c.latency_critical.cpu_wakeup_latency, 1000);
        assert_eq!(c.latency_critical.device_resume_latency, 1000);
        assert_eq!(c.throughput.cpu_wakeup_latency, 10000);
        assert_eq!(c.throughput.device_resume_latency, 100000);
    }

    #[test]
    fn latency_critical_floors_are_one_millisecond() {
        // Explicit contract-correction guard: this is the value that was
        // changed from 10/100 µs to 1000/1000 µs. If anyone reverts it,
        // this test fails loudly with the rationale in the assertion msg.
        let c = Contracts::default();
        assert_eq!(
            c.latency_critical.cpu_wakeup_latency, 1000,
            "latency-critical CPU wakeup floor must be 1 ms (1000 µs), not 10 µs"
        );
        assert_eq!(
            c.latency_critical.device_resume_latency, 1000,
            "latency-critical device-resume floor must be 1 ms (1000 µs), not 100 µs"
        );
    }

    #[test]
    fn load_published_contracts_toml_matches_default() {
        // The shipped config/optid/contracts.toml is the operator-facing
        // expression of the same defaults compiled into the binary. They
        // must agree byte-for-byte on every floor. This test reads the
        // actual published file (relative to the crate manifest) so a
        // contracts.toml edit without a contracts.rs edit (or vice versa)
        // fails here.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let published = Path::new(manifest_dir)
            .join("..")
            .join("..")
            .join("config")
            .join("optid")
            .join("contracts.toml");
        let loaded = Contracts::load(&published);
        let default = Contracts::default();
        assert_eq!(
            loaded.idle, default.idle,
            "idle floors drifted from default"
        );
        assert_eq!(
            loaded.light, default.light,
            "light floors drifted from default"
        );
        assert_eq!(
            loaded.interactive, default.interactive,
            "interactive floors drifted from default"
        );
        assert_eq!(
            loaded.latency_critical, default.latency_critical,
            "latency-critical floors drifted from default"
        );
        assert_eq!(
            loaded.throughput, default.throughput,
            "throughput floors drifted from default"
        );
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
            WorkloadClass::VmGuest,
        ] {
            let _ = c.resolve(class);
        }
    }

    #[test]
    fn vm_guest_resolve_returns_interactive_floors() {
        let c = Contracts::default();
        let vm_guest_floors = c.resolve(WorkloadClass::VmGuest);
        let interactive_floors = c.resolve(WorkloadClass::Interactive);
        assert_eq!(vm_guest_floors, interactive_floors);
        assert_eq!(vm_guest_floors.cpu_wakeup_latency, 1000);
        assert_eq!(vm_guest_floors.device_resume_latency, 10000);
    }

    #[test]
    fn vm_guest_resolve_does_not_return_latency_critical_floors() {
        let c = Contracts::default();
        let vm_guest_floors = c.resolve(WorkloadClass::VmGuest);
        let lc_floors = c.resolve(WorkloadClass::LatencyCritical);
        assert_ne!(vm_guest_floors, lc_floors);
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
