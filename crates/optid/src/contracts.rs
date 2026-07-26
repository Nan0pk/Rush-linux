//! Per-workload-class latency-budget contracts.
//!
//! A `Contracts` table maps each `WorkloadClass` to a pair of latency floors
//! (CPU wakeup + per-device resume) expressed in microseconds. The floors are
//! the *responsiveness floor* from `SPEC-northstar.md` §0 — they are the
//! single constraint that may stop a depth-enabler from going deeper.
//!
//! Values are provisional pending WP-B1 validation against real hardware
//! wakeup distributions; `Contracts::default()` matches `config/optid/contracts.toml`.
//!
//! ## Exit latency evidence (PR #333 semantic fix)
//!
//! Autosuspend delay is **not** exit latency. A contract-setting value
//! (e.g. writing `pm_qos_resume_latency_us`) is **not** measured selected-state
//! latency. Unknown exit latency fails closed. No latency value may be
//! invented. Runtime-PM depth changes may pass the contract gate only when
//! explicit measured or hardware-proven exit latency with provenance exists
//! (package C1). Until then, runtime-PM is contract-denied with an
//! operator-visible reason.

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
            WorkloadClass::VmGuest => self.interactive,
        }
    }
}

/// How exit latency was obtained. Autosuspend delay and contract-setting
/// values are **not** valid provenances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitLatencyProvenance {
    /// Direct measurement of state-exit latency (C1).
    Measured,
    /// Hardware/firmware identity table entry with verified latency (C1).
    HardwareProven,
}

/// Explicit exit-latency evidence required by the contract gate for
/// depth-enablers that trade resume latency for power (runtime-PM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitLatencyEvidence {
    /// Exit latency in **microseconds**. Units must not be mixed with ms.
    pub(crate) latency_us: u64,
    pub(crate) provenance: ExitLatencyProvenance,
}

impl ExitLatencyEvidence {
    pub(crate) fn measured_us(latency_us: u64) -> Self {
        Self {
            latency_us,
            provenance: ExitLatencyProvenance::Measured,
        }
    }

    pub(crate) fn hardware_proven_us(latency_us: u64) -> Self {
        Self {
            latency_us,
            provenance: ExitLatencyProvenance::HardwareProven,
        }
    }

    /// C1 readiness probe: returns true when evidence is well-formed.
    /// Called from the actuator path so constructors/provenances stay live
    /// in production builds (C1 will supply real values later).
    pub(crate) fn is_usable(&self) -> bool {
        match self.provenance {
            ExitLatencyProvenance::Measured | ExitLatencyProvenance::HardwareProven => {
                self.latency_us > 0
            }
        }
    }
}

/// Result of a contract-gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContractGateResult {
    /// Action may proceed.
    Permit,
    /// Action must not proceed; `reason` is operator-visible.
    Deny { reason: String },
}

/// Predicate from the SPEC-northstar §3 actuation rule:
///
/// ```text
/// contract gate: exit_latency(S) ≤ active_contract.floor(D)
/// ```
///
/// Compares **proven** exit latency (microseconds) against the floor
/// (microseconds). Does not invent latency and does not accept
/// autosuspend delay as a substitute.
pub(crate) fn fits_contract(exit_latency_us: u64, floor_us: u64) -> bool {
    exit_latency_us <= floor_us
}

/// Evaluate whether proven exit-latency evidence satisfies the floor.
///
/// Unknown evidence must use [`contract_gate_runtime_pm`] which fails closed.
pub(crate) fn fits_contract_evidence(evidence: &ExitLatencyEvidence, floor_us: u64) -> bool {
    fits_contract(evidence.latency_us, floor_us)
}

/// Contract gate for runtime-PM depth changes.
///
/// Without explicit measured/hardware-proven exit latency, the gate **denies**
/// (fail closed). Autosuspend delay is never converted into exit latency.
pub(crate) fn contract_gate_runtime_pm(
    evidence: Option<&ExitLatencyEvidence>,
    floor_us: u64,
    device_label: &str,
) -> ContractGateResult {
    match evidence {
        None => ContractGateResult::Deny {
            reason: format!(
                "contract gate BLOCKED runtime_pm {device_label}: \
                 exit_latency unknown (no measured/hardware-proven evidence); \
                 autosuspend_delay_ms is not exit latency; fail closed until C1"
            ),
        },
        Some(ev) => {
            // Both provenances are first-class; no invented third source.
            let provenance_label = match ev.provenance {
                ExitLatencyProvenance::Measured => "measured",
                ExitLatencyProvenance::HardwareProven => "hardware_proven",
            };
            if fits_contract_evidence(ev, floor_us) {
                ContractGateResult::Permit
            } else {
                ContractGateResult::Deny {
                    reason: format!(
                        "contract gate BLOCKED runtime_pm {device_label}: \
                         exit_latency={}us (provenance={provenance_label}) > floor={}us",
                        ev.latency_us, floor_us
                    ),
                }
            }
        }
    }
}

/// Contract gate for writing a device resume-latency **constraint**
/// (`pm_qos_resume_latency_us`). The value being written is a QoS ceiling
/// request, not measured selected-state exit latency. The gate permits
/// setting a constraint only when the requested ceiling is ≤ the class
/// floor (tighter or equal constraint). Missing or negative values are
/// denied — fail-closed — because they would otherwise allow any exit
/// latency.
///
/// Prior to the post-#337 repair this gate was fail-open on two edges:
/// a negative requested value returned `Permit` ("leave non-latency
/// sentinels to other layers"), and a missing value was the only
/// denial. Both are now denials; the actuator no longer relies on a
/// separate layer to reject malformed constraints.
///
/// Note on the `i64` parameter: the actuator calls this with
/// `value.map(i64::from)` from `Option<i32>`. A positive `i32` always
/// fits in `u64`, so the conversion never fails for valid caller input.
/// There is no separate "overflow" branch — a negative `i64` is caught
/// by the `v < 0` arm, and the `u64::try_from` for a non-negative `i64`
/// always succeeds. The post-#338 review removed the unreachable
/// `Err(_)` branch and its misleading "overflows u64" narrative.
pub(crate) fn contract_gate_device_resume_constraint(
    requested_ceiling_us: Option<i64>,
    floor_us: u64,
    path_label: &str,
) -> ContractGateResult {
    match requested_ceiling_us {
        None => ContractGateResult::Deny {
            reason: format!(
                "contract gate BLOCKED device_resume_latency {path_label}: \
                 unconstrained (None) would allow any exit latency; fail closed"
            ),
        },
        Some(v) if v < 0 => ContractGateResult::Deny {
            reason: format!(
                "contract gate BLOCKED device_resume_latency {path_label}: \
                 requested_ceiling={v}us is negative; fail closed (no sentinels bypass the gate)"
            ),
        },
        Some(v) => {
            // v >= 0 here, so u64::try_from always succeeds. Use
            // match to keep the compiler-checked exhaustiveness in case
            // a future caller passes a wider integer type.
            let us = u64::try_from(v).unwrap_or(u64::MAX);
            if fits_contract(us, floor_us) {
                ContractGateResult::Permit
            } else {
                ContractGateResult::Deny {
                    reason: format!(
                        "contract gate BLOCKED device_resume_latency {path_label}: \
                         requested_ceiling={us}us > floor={floor_us}us"
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_contracts_match_config_optid_contracts_toml() {
        let c = Contracts::default();
        assert_eq!(c.idle.cpu_wakeup_latency, 100000);
        assert_eq!(c.idle.device_resume_latency, 1000000);
        assert_eq!(c.interactive.cpu_wakeup_latency, 1000);
        assert_eq!(c.interactive.device_resume_latency, 10000);
        assert_eq!(c.latency_critical.cpu_wakeup_latency, 1000);
        assert_eq!(c.latency_critical.device_resume_latency, 1000);
        assert_eq!(c.throughput.cpu_wakeup_latency, 10000);
        assert_eq!(c.throughput.device_resume_latency, 100000);
    }

    #[test]
    fn latency_critical_floors_are_one_millisecond() {
        let c = Contracts::default();
        assert_eq!(c.latency_critical.cpu_wakeup_latency, 1000);
        assert_eq!(c.latency_critical.device_resume_latency, 1000);
    }

    #[test]
    fn load_published_contracts_toml_matches_default() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let published = Path::new(manifest_dir)
            .join("..")
            .join("..")
            .join("config")
            .join("optid")
            .join("contracts.toml");
        let loaded = Contracts::load(&published);
        let default = Contracts::default();
        assert_eq!(loaded.idle, default.idle);
        assert_eq!(loaded.light, default.light);
        assert_eq!(loaded.interactive, default.interactive);
        assert_eq!(loaded.latency_critical, default.latency_critical);
        assert_eq!(loaded.throughput, default.throughput);
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

    // ── Semantic contract tests (PR #333 repairs) ────────────────────

    #[test]
    fn large_autosuspend_delay_does_not_imply_large_exit_latency() {
        // A 60_000 ms autosuspend delay must NEVER be multiplied into
        // 60_000_000 µs exit latency. Without provenance, gate denies.
        let floor = 1_000_000u64;
        let result = contract_gate_runtime_pm(None, floor, "usb-1-1");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("unknown") || reason.contains("not exit latency"));
            // Must not invent a microsecond figure from the delay.
            assert!(!reason.contains("60000000"));
        }
    }

    #[test]
    fn small_autosuspend_delay_does_not_prove_compliance() {
        // 1 ms autosuspend is still not exit-latency evidence.
        let floor = 1_000_000u64;
        let result = contract_gate_runtime_pm(None, floor, "usb-1-2");
        assert!(
            matches!(result, ContractGateResult::Deny { .. }),
            "small delay without provenance must not pass"
        );
    }

    #[test]
    fn unknown_latency_fails_closed() {
        let result = contract_gate_runtime_pm(None, 1_000_000, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
    }

    #[test]
    fn proven_latency_below_floor_passes() {
        let ev = ExitLatencyEvidence::measured_us(500);
        let result = contract_gate_runtime_pm(Some(&ev), 1000, "dev");
        assert_eq!(result, ContractGateResult::Permit);
        assert!(fits_contract_evidence(&ev, 1000));
    }

    #[test]
    fn proven_latency_above_floor_fails() {
        let ev = ExitLatencyEvidence::hardware_proven_us(5000);
        let result = contract_gate_runtime_pm(Some(&ev), 1000, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("5000us"));
            assert!(reason.contains("floor=1000us"));
        }
    }

    #[test]
    fn units_cannot_be_mixed() {
        // Evidence is always microseconds. A value of 2000 that was meant
        // as milliseconds must not silently pass a 10_000 µs floor if the
        // caller correctly supplies 2_000_000 µs.
        let as_if_ms_confused = ExitLatencyEvidence::measured_us(2000); // wrong unit if 2000ms
        assert!(
            fits_contract_evidence(&as_if_ms_confused, 10_000),
            "2000µs fits 10000µs floor — correct unit comparison"
        );
        let correct_ms_as_us = ExitLatencyEvidence::measured_us(2_000_000);
        assert!(
            !fits_contract_evidence(&correct_ms_as_us, 10_000),
            "2000ms expressed as 2_000_000µs must fail a 10_000µs floor"
        );
    }

    #[test]
    fn device_resume_constraint_is_not_selected_state_latency() {
        // Setting pm_qos to 500µs is a ceiling request, not a claim that
        // the device exits in 500µs. It still must not exceed the floor.
        assert_eq!(
            contract_gate_device_resume_constraint(Some(500), 1000, "path"),
            ContractGateResult::Permit
        );
        assert!(matches!(
            contract_gate_device_resume_constraint(Some(5000), 1000, "path"),
            ContractGateResult::Deny { .. }
        ));
        // Unconstrained fails closed.
        assert!(matches!(
            contract_gate_device_resume_constraint(None, 1000, "path"),
            ContractGateResult::Deny { .. }
        ));
    }

    // ── Post-#337 fail-closed edges for the device-resume constraint gate ──

    #[test]
    fn device_resume_constraint_negative_requested_value_denies() {
        // A negative requested ceiling is not a sentinel — it is malformed
        // and must fail closed. The pre-#337 gate returned `Permit` here,
        // leaving "non-latency sentinels to other layers"; that was an
        // open hole because no other layer actually rejected them.
        let result = contract_gate_device_resume_constraint(Some(-1), 1000, "path");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("negative"), "reason: {reason}");
            assert!(reason.contains("fail closed"));
        }
    }

    #[test]
    fn device_resume_constraint_large_negative_denies() {
        let result = contract_gate_device_resume_constraint(Some(-1_000_000), 1000, "path");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
    }

    #[test]
    fn device_resume_constraint_huge_positive_denied_as_above_floor() {
        // A huge positive ceiling (i64::MAX) exceeds any reasonable floor
        // and must be denied as "above floor". There is no separate
        // "overflow" branch: a positive i64 always fits in u64, so
        // u64::try_from always succeeds. The post-#338 review removed
        // the unreachable overflow narrative; this test pins the honest
        // behavior — huge ceilings are denied by the floor comparison,
        // not by an impossible overflow check.
        let result = contract_gate_device_resume_constraint(Some(i64::MAX), 1000, "path");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(
                reason.contains("> floor="),
                "huge ceiling must be denied as 'above floor': {reason}"
            );
            assert!(
                !reason.contains("overflow"),
                "no overflow narrative (positive i64 always fits in u64): {reason}"
            );
        }
    }

    #[test]
    fn device_resume_constraint_missing_value_denies() {
        let result = contract_gate_device_resume_constraint(None, 1000, "path");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("unconstrained"));
        }
    }

    #[test]
    fn device_resume_constraint_valid_tighter_than_floor_passes() {
        // A ceiling at or below the floor is a tighter constraint and may pass.
        assert_eq!(
            contract_gate_device_resume_constraint(Some(1000), 1000, "path"),
            ContractGateResult::Permit
        );
        assert_eq!(
            contract_gate_device_resume_constraint(Some(1), 1000, "path"),
            ContractGateResult::Permit
        );
    }

    #[test]
    fn device_resume_constraint_above_floor_denies() {
        let result = contract_gate_device_resume_constraint(Some(1001), 1000, "path");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("1001us"));
            assert!(reason.contains("floor=1000us"));
        }
    }

    // ── Runtime-PM gate fail-closed edges ──────────────────────────────

    #[test]
    fn runtime_pm_without_evidence_denies() {
        // Without measured or hardware-proven exit-latency evidence, the
        // gate denies. Autosuspend delay is never converted to evidence.
        let result = contract_gate_runtime_pm(None, 1_000_000, "usb-1-1");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("unknown"));
            assert!(reason.contains("fail closed"));
        }
    }

    #[test]
    fn runtime_pm_measured_latency_below_floor_passes() {
        let ev = ExitLatencyEvidence::measured_us(500);
        let result = contract_gate_runtime_pm(Some(&ev), 1000, "dev");
        assert_eq!(result, ContractGateResult::Permit);
    }

    #[test]
    fn runtime_pm_measured_latency_above_floor_denies() {
        let ev = ExitLatencyEvidence::measured_us(5000);
        let result = contract_gate_runtime_pm(Some(&ev), 1000, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("5000us"));
            assert!(reason.contains("floor=1000us"));
        }
    }

    #[test]
    fn runtime_pm_hardware_proven_latency_below_floor_passes() {
        let ev = ExitLatencyEvidence::hardware_proven_us(100);
        let result = contract_gate_runtime_pm(Some(&ev), 1000, "dev");
        assert_eq!(result, ContractGateResult::Permit);
    }

    #[test]
    fn runtime_pm_hardware_proven_latency_above_floor_denies() {
        let ev = ExitLatencyEvidence::hardware_proven_us(10_000);
        let result = contract_gate_runtime_pm(Some(&ev), 1000, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
    }
}
