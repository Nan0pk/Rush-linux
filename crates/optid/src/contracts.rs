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
//! ## Exit latency evidence (PR #333 semantic fix, completed by C1)
//!
//! Autosuspend delay is **not** exit latency. A contract-setting value
//! (e.g. writing `pm_qos_resume_latency_us`) is **not** measured selected-state
//! latency. Unknown exit latency fails closed. No latency value may be
//! invented.
//!
//! C1 supplies the missing half: [`crate::latency::LatencyEstimate`] carries a
//! value with its provenance, and [`ContractEvaluator`] compares it against the
//! floor. A runtime-PM depth change passes only when a verified estimate
//! resolves for the device and fits the floor; every other case resolves to
//! [`crate::latency::LatencyResolution::Unknown`] and is denied with an
//! operator-visible reason.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::latency::LatencyResolution;
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

/// Result of a contract-gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContractGateResult {
    /// Action may proceed.
    Permit,
    /// Action must not proceed; `reason` is operator-visible.
    Deny { reason: String },
    /// The gate would have denied, but `[contracts] mode = "observe"` is set
    /// *and* the write is not real, so the dry run may proceed and record
    /// what enforcement would have done.
    ///
    /// This variant is never produced when a real write is armed — see
    /// [`ContractEvaluator`]. Observe mode reports; it does not disarm.
    ObserveOnly { would_deny_reason: String },
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

/// Whether the contract gate enforces its verdict or only reports it.
///
/// `enforce` is the default and the only mode that may be active for a real
/// write. `observe` exists so an operator can see what enforcement *would*
/// block during rollout without a depth-enabler silently going deeper than
/// the responsiveness floor allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContractsMode {
    #[default]
    Enforce,
    Observe,
}

impl ContractsMode {
    /// Parse `[contracts] mode`. An unrecognised value falls back to
    /// `Enforce` — a typo must never silently relax the gate.
    pub(crate) fn parse(raw: &str) -> Self {
        match raw.trim().trim_matches('"').trim_matches('\'') {
            "observe" => ContractsMode::Observe,
            _ => ContractsMode::Enforce,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ContractsMode::Enforce => "enforce",
            ContractsMode::Observe => "observe",
        }
    }
}

/// The full set of contracts in force: the per-class base table plus any
/// per-cgroup-scope overrides, and the mode the gate runs in.
///
/// ## Composition rule
///
/// When more than one contract applies to a device — the committed workload
/// class always, plus every active scope override that names it — the
/// effective floor is the **strictest** (numerically smallest) of them.
/// A floor is the largest latency the workload tolerates, so the tightest
/// constraint has to win; taking anything looser would let one scope's
/// override raise another scope's ceiling, which is how a latency-critical
/// workload ends up behind a device that takes a millisecond to wake.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ContractBook {
    base: Contracts,
    /// Floors keyed by cgroup scope name, from `[contracts.cgroup."<scope>"]`.
    cgroup_overrides: BTreeMap<String, ContractFloors>,
    mode: ContractsMode,
}

impl ContractBook {
    /// Load the base table, the per-cgroup overrides, and the mode from one
    /// contracts file. Missing or malformed input degrades to defaults for
    /// the same reason `Contracts::load` does: a corrupt contracts file must
    /// lose overrides, never break the daemon.
    pub(crate) fn load(path: &Path) -> Self {
        let base = Contracts::load(path);
        let mut cgroup_overrides: BTreeMap<String, ContractFloors> = BTreeMap::new();
        let mut mode = ContractsMode::default();

        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => {
                return Self {
                    base,
                    cgroup_overrides,
                    mode,
                }
            }
        };

        let mut scope: Option<String> = None;
        let mut in_contracts_root = false;
        for line in text.lines() {
            let line = strip_comment(line);
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let section = line[1..line.len() - 1].trim();
                in_contracts_root = section == "contracts";
                scope = section
                    .strip_prefix("contracts.cgroup.")
                    .map(|raw| raw.trim().trim_matches('"').to_string())
                    .filter(|name| !name.is_empty());
                continue;
            }

            let mut parts = line.splitn(2, '=');
            let (Some(key), Some(val)) = (parts.next(), parts.next()) else {
                continue;
            };
            let (key, val) = (key.trim(), val.trim());

            if in_contracts_root && key == "mode" {
                mode = ContractsMode::parse(val);
                continue;
            }

            let Some(scope_name) = scope.as_ref() else {
                continue;
            };
            let Ok(parsed) = val.parse::<i64>() else {
                continue;
            };
            // An override that names only one of the two floors inherits the
            // other from the interactive base row rather than defaulting to
            // zero, which `effective_floors` would reject as invalid.
            let entry = cgroup_overrides
                .entry(scope_name.clone())
                .or_insert(base.interactive);
            match key {
                "cpu_wakeup_latency" => entry.cpu_wakeup_latency = parsed,
                "device_resume_latency" => entry.device_resume_latency = parsed,
                _ => {}
            }
        }

        Self {
            base,
            cgroup_overrides,
            mode,
        }
    }

    pub(crate) fn mode(&self) -> ContractsMode {
        self.mode
    }

    pub(crate) fn base(&self) -> &Contracts {
        &self.base
    }

    /// The floors in force for `class` once every override naming an active
    /// scope has been composed in. See the type-level composition rule.
    ///
    /// `active_scopes` is the set of cgroup scopes currently driving the
    /// decision. Scope discovery is package O2; until it lands the daemon
    /// passes an empty slice and this reduces to the base class row.
    pub(crate) fn effective_floors(
        &self,
        class: WorkloadClass,
        active_scopes: &[String],
    ) -> ContractFloors {
        let mut floors = self.base.resolve(class);
        for scope in active_scopes {
            let Some(override_floors) = self.cgroup_overrides.get(scope) else {
                continue;
            };
            floors = tighter_of(floors, *override_floors);
        }
        floors
    }
}

/// Strip a trailing `#` comment and surrounding whitespace from one line.
fn strip_comment(line: &str) -> &str {
    let line = line.trim();
    match line.find('#') {
        Some(idx) => line[..idx].trim(),
        None => line,
    }
}

/// Compose two contracts by keeping the strictest floor on each axis.
///
/// A non-positive floor is not a looser constraint, it is an invalid one, so
/// it never wins the comparison — the actuator rejects an invalid floor
/// separately and this must not hand it one it would otherwise not have seen.
fn tighter_of(a: ContractFloors, b: ContractFloors) -> ContractFloors {
    ContractFloors {
        cpu_wakeup_latency: tighter_axis(a.cpu_wakeup_latency, b.cpu_wakeup_latency),
        device_resume_latency: tighter_axis(a.device_resume_latency, b.device_resume_latency),
    }
}

fn tighter_axis(a: i64, b: i64) -> i64 {
    match (a > 0, b > 0) {
        (true, true) => a.min(b),
        (true, false) => a,
        (false, true) => b,
        (false, false) => a,
    }
}

/// The pure C1 contract evaluator.
///
/// Takes a resolved exit latency, a floor, and whether the write is real, and
/// returns the gate verdict. No I/O, no clock, no global state — the same
/// inputs always give the same verdict, which is what makes the property
/// test in this module meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ContractEvaluator {
    mode: ContractsMode,
}

impl ContractEvaluator {
    pub(crate) fn new(mode: ContractsMode) -> Self {
        Self { mode }
    }

    /// Evaluate a depth-enabling action against the floor.
    ///
    /// `applying` is true when the caller is armed to perform a real write.
    /// Observe mode only relaxes a denial when it is false: SPEC's rule is
    /// that `enforce` is the mode "for apply", so an operator who leaves
    /// `mode = "observe"` set and then arms `--apply` gets enforcement, not a
    /// disarmed gate. Fail-open on a live write is the one outcome this gate
    /// exists to prevent.
    pub(crate) fn evaluate_depth(
        &self,
        resolution: &LatencyResolution,
        floor_us: u64,
        applying: bool,
        device_label: &str,
    ) -> ContractGateResult {
        let deny_reason = match resolution {
            LatencyResolution::Unknown { reason } => Some(format!(
                "contract gate BLOCKED runtime_pm {device_label}: exit latency unknown \
                 ({reason}); autosuspend_delay_ms is not exit latency and a pm_qos constraint \
                 is not a measurement"
            )),
            LatencyResolution::Known(estimate) => {
                if fits_contract(estimate.value_us, floor_us) {
                    None
                } else {
                    Some(format!(
                        "contract gate BLOCKED runtime_pm {device_label}: exit_latency={} > \
                         floor={floor_us}us",
                        estimate.describe()
                    ))
                }
            }
        };

        match deny_reason {
            None => ContractGateResult::Permit,
            Some(reason) => match (self.mode, applying) {
                (ContractsMode::Observe, false) => ContractGateResult::ObserveOnly {
                    would_deny_reason: reason,
                },
                _ => ContractGateResult::Deny { reason },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latency::{LatencyConfidence, LatencyEstimate, LatencySource};

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

    // ── Semantic contract tests (PR #333 repairs, C1 evaluator) ─────
    //
    // These kept their meaning across C1: the placeholder
    // `contract_gate_runtime_pm` was replaced by `ContractEvaluator`, which
    // takes a `LatencyResolution` instead of an `Option<ExitLatencyEvidence>`.
    // The behaviour under test — that a delay is never evidence, and that
    // unknown fails closed — is unchanged.

    /// A `Known` resolution at `value_us`, as an allowlist-verified estimate.
    fn known_us(value_us: u64) -> LatencyResolution {
        LatencyResolution::Known(LatencyEstimate {
            value_us,
            source: LatencySource::AllowlistVerified,
            confidence: LatencyConfidence::Medium,
            measured_at: None,
            hardware_id: "pci:v0000TESTd0000TEST".to_string(),
            firmware_id: None,
        })
    }

    fn unknown() -> LatencyResolution {
        LatencyResolution::unknown("no evidence in test")
    }

    fn enforcing() -> ContractEvaluator {
        ContractEvaluator::new(ContractsMode::Enforce)
    }

    #[test]
    fn large_autosuspend_delay_does_not_imply_large_exit_latency() {
        // A 60_000 ms autosuspend delay must NEVER be multiplied into
        // 60_000_000 µs exit latency. Without provenance, gate denies.
        let floor = 1_000_000u64;
        let result = enforcing().evaluate_depth(&unknown(), floor, true, "usb-1-1");
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
        let result = enforcing().evaluate_depth(&unknown(), 1_000_000, true, "usb-1-2");
        assert!(
            matches!(result, ContractGateResult::Deny { .. }),
            "small delay without provenance must not pass"
        );
    }

    #[test]
    fn unknown_latency_fails_closed() {
        let result = enforcing().evaluate_depth(&unknown(), 1_000_000, true, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
    }

    #[test]
    fn proven_latency_below_floor_passes() {
        let result = enforcing().evaluate_depth(&known_us(500), 1000, true, "dev");
        assert_eq!(result, ContractGateResult::Permit);
    }

    #[test]
    fn proven_latency_above_floor_fails() {
        let result = enforcing().evaluate_depth(&known_us(5000), 1000, true, "dev");
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
        assert_eq!(
            enforcing().evaluate_depth(&known_us(2000), 10_000, true, "dev"),
            ContractGateResult::Permit,
            "2000us fits a 10000us floor — correct unit comparison"
        );
        assert!(
            matches!(
                enforcing().evaluate_depth(&known_us(2_000_000), 10_000, true, "dev"),
                ContractGateResult::Deny { .. }
            ),
            "2000ms expressed as 2_000_000us must fail a 10_000us floor"
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
        // Without verified exit-latency evidence, the gate denies.
        // Autosuspend delay is never converted to evidence.
        let result = enforcing().evaluate_depth(&unknown(), 1_000_000, true, "usb-1-1");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("unknown"));
            assert!(reason.contains("autosuspend_delay_ms is not exit latency"));
        }
    }

    #[test]
    fn runtime_pm_measured_latency_below_floor_passes() {
        let result = enforcing().evaluate_depth(&known_us(500), 1000, true, "dev");
        assert_eq!(result, ContractGateResult::Permit);
    }

    #[test]
    fn runtime_pm_measured_latency_above_floor_denies() {
        let result = enforcing().evaluate_depth(&known_us(5000), 1000, true, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
        if let ContractGateResult::Deny { reason } = result {
            assert!(reason.contains("5000us"));
            assert!(reason.contains("floor=1000us"));
        }
    }

    #[test]
    fn runtime_pm_hardware_proven_latency_below_floor_passes() {
        let result = enforcing().evaluate_depth(&known_us(100), 1000, true, "dev");
        assert_eq!(result, ContractGateResult::Permit);
    }

    #[test]
    fn runtime_pm_hardware_proven_latency_above_floor_denies() {
        let result = enforcing().evaluate_depth(&known_us(10_000), 1000, true, "dev");
        assert!(matches!(result, ContractGateResult::Deny { .. }));
    }

    // ── C1: contract book, composition, mode, and the evaluator ────────

    fn book_from(text: &str) -> ContractBook {
        let tmp = std::env::temp_dir().join(format!(
            "optid_c1_book_{}_{}",
            std::process::id(),
            text.len()
        ));
        std::fs::write(&tmp, text).expect("write temp contracts file");
        let book = ContractBook::load(&tmp);
        let _ = std::fs::remove_file(&tmp);
        book
    }

    #[test]
    fn c1_contract_book_parses_per_cgroup_overrides() {
        let book = book_from(
            "[contracts.interactive]\n\
             cpu_wakeup_latency = 1000\n\
             device_resume_latency = 10000\n\
             [contracts.cgroup.\"app.slice/game.scope\"]\n\
             device_resume_latency = 400\n",
        );
        let scope = vec!["app.slice/game.scope".to_string()];
        let floors = book.effective_floors(WorkloadClass::Interactive, &scope);
        assert_eq!(
            floors.device_resume_latency, 400,
            "the scope override must apply when its scope is active"
        );
        assert_eq!(
            book.effective_floors(WorkloadClass::Interactive, &[])
                .device_resume_latency,
            10000,
            "with no active scope the base class row stands"
        );
    }

    #[test]
    fn c1_inactive_scope_override_does_not_apply() {
        let book = book_from(
            "[contracts.interactive]\n\
             device_resume_latency = 10000\n\
             [contracts.cgroup.\"app.slice/game.scope\"]\n\
             device_resume_latency = 400\n",
        );
        let other = vec!["app.slice/editor.scope".to_string()];
        assert_eq!(
            book.effective_floors(WorkloadClass::Interactive, &other)
                .device_resume_latency,
            10000
        );
    }

    #[test]
    fn c1_multiple_active_contracts_compose_to_the_strictest() {
        let book = book_from(
            "[contracts.interactive]\n\
             device_resume_latency = 10000\n\
             [contracts.cgroup.\"a.scope\"]\n\
             device_resume_latency = 5000\n\
             [contracts.cgroup.\"b.scope\"]\n\
             device_resume_latency = 250\n\
             [contracts.cgroup.\"c.scope\"]\n\
             device_resume_latency = 4000\n",
        );
        let scopes = vec![
            "a.scope".to_string(),
            "b.scope".to_string(),
            "c.scope".to_string(),
        ];
        assert_eq!(
            book.effective_floors(WorkloadClass::Interactive, &scopes)
                .device_resume_latency,
            250,
            "the tightest active contract wins; a looser one must not raise the ceiling"
        );
    }

    #[test]
    fn c1_a_looser_override_cannot_relax_the_class_floor() {
        // The composition rule is one-directional. An override that asks for
        // a *larger* tolerance than the class contract must not win, or a
        // per-app config could quietly opt out of the responsiveness floor.
        let book = book_from(
            "[contracts.latency-critical]\n\
             device_resume_latency = 1000\n\
             [contracts.cgroup.\"greedy.scope\"]\n\
             device_resume_latency = 900000\n",
        );
        let scope = vec!["greedy.scope".to_string()];
        assert_eq!(
            book.effective_floors(WorkloadClass::LatencyCritical, &scope)
                .device_resume_latency,
            1000
        );
    }

    #[test]
    fn c1_contracts_mode_defaults_to_enforce_and_parses_observe() {
        assert_eq!(
            book_from("[contracts.idle]\n").mode(),
            ContractsMode::Enforce
        );
        assert_eq!(
            book_from("[contracts]\nmode = \"observe\"\n").mode(),
            ContractsMode::Observe
        );
        assert_eq!(
            book_from("[contracts]\nmode = \"enforce\"\n").mode(),
            ContractsMode::Enforce
        );
        assert_eq!(
            book_from("[contracts]\nmode = \"obsrve\"\n").mode(),
            ContractsMode::Enforce,
            "a typo must fail closed to enforce, never relax the gate"
        );
    }

    #[test]
    fn c1_observe_mode_reports_but_never_relaxes_a_real_write() {
        let evaluator = ContractEvaluator::new(ContractsMode::Observe);

        let dry = evaluator.evaluate_depth(&unknown(), 1000, false, "dev");
        match dry {
            ContractGateResult::ObserveOnly { would_deny_reason } => {
                assert!(would_deny_reason.contains("unknown"))
            }
            other => panic!("observe + dry run should report, got {other:?}"),
        }

        let armed = evaluator.evaluate_depth(&unknown(), 1000, true, "dev");
        assert!(
            matches!(armed, ContractGateResult::Deny { .. }),
            "observe mode must not disarm the gate once a real write is armed"
        );
    }

    #[test]
    fn c1_boundary_equality_permits_exactly_at_the_floor() {
        // `exit_latency <= floor` — equality is inside the contract.
        assert_eq!(
            enforcing().evaluate_depth(&known_us(1000), 1000, true, "dev"),
            ContractGateResult::Permit
        );
        assert!(matches!(
            enforcing().evaluate_depth(&known_us(1001), 1000, true, "dev"),
            ContractGateResult::Deny { .. }
        ));
    }

    #[test]
    fn c1_denial_reason_names_the_provenance_and_the_floor() {
        let result = enforcing().evaluate_depth(&known_us(4200), 1000, true, "pci-0000:04:00.0");
        match result {
            ContractGateResult::Deny { reason } => {
                assert!(reason.contains("pci-0000:04:00.0"), "reason: {reason}");
                assert!(reason.contains("4200us"), "reason: {reason}");
                assert!(
                    reason.contains("source=allowlist_verified"),
                    "reason: {reason}"
                );
                assert!(reason.contains("floor=1000us"), "reason: {reason}");
            }
            other => panic!("expected a denial, got {other:?}"),
        }
    }

    #[test]
    fn c1_unknown_denial_reason_rejects_the_two_lookalike_values() {
        match enforcing().evaluate_depth(&unknown(), 1000, true, "dev") {
            ContractGateResult::Deny { reason } => {
                assert!(reason.contains("autosuspend_delay_ms is not exit latency"));
                assert!(reason.contains("pm_qos constraint is not a measurement"));
            }
            other => panic!("expected a denial, got {other:?}"),
        }
    }

    /// Property: tightening a floor can only ever remove permissions.
    ///
    /// This is the safety property the whole gate exists to hold. If a
    /// tighter floor could authorize a state a looser floor denied, the
    /// contract would be meaningless — so sweep the space and assert
    /// monotonicity directly.
    #[test]
    fn c1_property_tightening_a_floor_never_authorizes_a_deeper_state() {
        let evaluator = enforcing();
        for latency in [0u64, 1, 250, 999, 1000, 1001, 5_000, 1_000_000] {
            let resolution = known_us(latency);
            let mut previous_permitted = true;
            // Walk floors from loosest to tightest.
            for floor in [1_000_000u64, 5_000, 1001, 1000, 999, 250, 1] {
                let permitted = matches!(
                    evaluator.evaluate_depth(&resolution, floor, true, "dev"),
                    ContractGateResult::Permit
                );
                assert!(
                    previous_permitted || !permitted,
                    "latency={latency}us became permitted at the tighter floor={floor}us \
                     after being denied at a looser one"
                );
                previous_permitted = permitted;
            }
        }
    }

    #[test]
    fn c1_property_unknown_is_denied_at_every_floor() {
        let evaluator = enforcing();
        for floor in [1u64, 1000, 1_000_000, u64::MAX] {
            assert!(
                matches!(
                    evaluator.evaluate_depth(&unknown(), floor, true, "dev"),
                    ContractGateResult::Deny { .. }
                ),
                "unknown latency must be denied even by the loosest floor ({floor}us)"
            );
        }
    }
}
