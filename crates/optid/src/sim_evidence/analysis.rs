//! Turning observations into an answer.
//!
//! Every judgement in this module is derived from two independent sources: the
//! simulated machine's own write log and control values (what actually
//! happened to the machine) and optid's control-cycle envelope (what optid says
//! happened). A disagreement between them is a rejection, not a rounding
//! detail.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;

use super::machine::{MachineSpec, SimFault, SimMachine, StepSample, WriteRecord, WriteResult};
use super::model::{evaluate_step, Assumptions};
use super::scenarios::{Arm, ArmKind, Scenario, KERNEL_DOMAINS, SYSTEMD_DOMAIN};
use super::Trial;

/// Relative change below which a difference is reported as "no meaningful
/// difference" rather than as an improvement or a regression.
pub(crate) const MEANINGFUL_RELATIVE_DELTA: f64 = 0.02;

const BASELINE_ARM: &str = "off_absent";

/// Metrics where a smaller number is better.
const LOWER_IS_BETTER: [&str; 8] = [
    "foreground_p99_latency_us",
    "foreground_mean_latency_us",
    "cpu_stall_pct",
    "memory_stall_pct",
    "io_stall_pct",
    "energy_j",
    "mean_power_w",
    "peak_die_temp_c",
];

/// Metrics where a larger number is better.
const HIGHER_IS_BETTER: [&str; 2] = ["throughput_ops_per_s", "completed_work_units"];

fn metric_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = LOWER_IS_BETTER.to_vec();
    names.extend(HIGHER_IS_BETTER);
    names.push("mean_die_temp_c");
    names
}

fn lower_is_better(metric: &str) -> Option<bool> {
    if LOWER_IS_BETTER.contains(&metric) {
        Some(true)
    } else if HIGHER_IS_BETTER.contains(&metric) {
        Some(false)
    } else {
        None
    }
}

/// What a receipt says about one control optid tried to change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceiptClass {
    /// The write landed and the control read back the requested value.
    ActiveAndRestored,
    /// The write landed and read back, but restoration did not return the
    /// previous value.
    ActiveNotRestored,
    /// The write was accepted and the control never changed. Unsupported.
    InertControl,
    /// The kernel refused the write.
    WriteRefused,
    /// The value the control ended on is not the value that was requested.
    ReadBackMismatch,
    /// A third party changed the control while optid owned it. optid
    /// relinquishes such a target by design, so it is not a restore failure.
    ExternallyDrifted,
    /// A write was recorded with no observable previous value.
    IncompleteReceipt,
}

/// The four-value record the requirement asks for, taken from the machine
/// rather than from optid's own claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Receipt {
    pub(crate) control_id: String,
    pub(crate) domain: String,
    pub(crate) previous_value: String,
    pub(crate) requested_value: String,
    pub(crate) read_back_value: String,
    pub(crate) restored_value: String,
    pub(crate) became_active: bool,
    pub(crate) restored_correctly: bool,
    pub(crate) write_attempts: u32,
    pub(crate) classification: ReceiptClass,
}

/// What optid's own control-cycle envelope reported.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct EnvelopeSummary {
    pub(crate) cycles: u32,
    pub(crate) schema_versions: BTreeSet<u32>,
    pub(crate) domains_seen: BTreeSet<String>,
    pub(crate) domain_support: BTreeMap<String, String>,
    pub(crate) domain_modes: BTreeMap<String, String>,
    pub(crate) targets_written: BTreeSet<String>,
    pub(crate) target_reasons: BTreeMap<String, String>,
    pub(crate) gate_denials: BTreeMap<String, String>,
    pub(crate) restore_targets: BTreeMap<String, String>,
    pub(crate) parse_errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MachineSummary {
    pub(crate) name: String,
    pub(crate) cpus: u32,
    pub(crate) devices: Vec<String>,
    pub(crate) sata_hosts: Vec<String>,
    pub(crate) backlight: String,
    pub(crate) note: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ScenarioSummary {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) safety_only: bool,
    pub(crate) cycles: u32,
    pub(crate) step_seconds: u64,
    pub(crate) workload: String,
    pub(crate) starts_on_ac: bool,
    pub(crate) faults: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DeterminismReport {
    pub(crate) repeats: u32,
    pub(crate) compared_groups: usize,
    pub(crate) identical_groups: usize,
    pub(crate) divergent: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Improved,
    NoMeaningfulDifference,
    Worse,
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Comparison {
    pub(crate) arm: String,
    pub(crate) scenario: String,
    pub(crate) metric: String,
    pub(crate) baseline_value: f64,
    pub(crate) arm_value: f64,
    pub(crate) relative_delta: f64,
    /// The change in the metric's own units. This is the honest figure when the
    /// baseline is smaller than the metric's noise floor.
    pub(crate) absolute_delta: f64,
    /// `true` when the baseline is below the metric's noise floor, so the
    /// relative figure is scaled by the floor rather than by the baseline.
    pub(crate) baseline_below_floor: bool,
    pub(crate) direction: Direction,
    /// `true` when every assumption set in the grid agrees on the direction.
    pub(crate) direction_stable: bool,
    pub(crate) sensitivity_range: (f64, f64),
    pub(crate) note: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Attribution {
    pub(crate) scenario: String,
    pub(crate) metric: String,
    pub(crate) domain: String,
    pub(crate) relative_delta: f64,
    pub(crate) direction: Direction,
    pub(crate) direction_stable: bool,
    pub(crate) actions: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MaskingFinding {
    pub(crate) scenario: String,
    pub(crate) metric: String,
    pub(crate) domain: String,
    pub(crate) isolated_relative_delta: f64,
    pub(crate) combined_relative_delta: f64,
    pub(crate) detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ControlReport {
    pub(crate) no_change_control_arms: Vec<String>,
    pub(crate) no_change_control_held: bool,
    pub(crate) no_change_violations: Vec<String>,
    pub(crate) harmful_control_arm: String,
    pub(crate) harmful_control_detected: bool,
    pub(crate) harmful_control_evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SafetyReport {
    pub(crate) total_receipts: usize,
    pub(crate) active_receipts: usize,
    pub(crate) restored_receipts: usize,
    /// An action that became active and did not return to its previous value in
    /// a scenario with no injected fault. This is the case that must never
    /// happen; it blocks the verdict.
    pub(crate) unrestored_without_a_fault: Vec<String>,
    /// The same, in a scenario whose injected fault explains it — a refused
    /// handback write, a truncated write, a crash. Reported, not blocking.
    pub(crate) unrestored_under_an_injected_fault: Vec<String>,
    pub(crate) inert_controls: Vec<String>,
    pub(crate) refused_writes: Vec<String>,
    pub(crate) crash_recovery_ran: bool,
    pub(crate) crash_recovery_restored_everything: bool,
    pub(crate) circuit_opened_in: Vec<String>,
    pub(crate) failed_restoration_detected: bool,
    pub(crate) containment_clean: bool,
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SupportReport {
    pub(crate) supported_domains: Vec<String>,
    /// Domains that produced an action which actually became active, per arm,
    /// counting only scenarios with no injected fault and no policy reload. The
    /// fully enabled arm carries an administrator allowlist override; the stock
    /// arm is the shipped seeded baseline.
    pub(crate) active_domains_by_arm: BTreeMap<String, Vec<String>>,
    /// Domains that became active in an arm that configures them `off` or
    /// `observe`, which can only happen through the policy-reload fallback.
    pub(crate) escalated_domains_by_arm: BTreeMap<String, Vec<String>>,
    pub(crate) unsupported_domains: BTreeMap<String, String>,
    pub(crate) rejected_results: Vec<String>,
}

/// A behaviour the simulation surfaced that is worth reporting on its own,
/// independently of whether optid helped or hurt the modelled workload.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Finding {
    pub(crate) id: String,
    pub(crate) severity: String,
    pub(crate) summary: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Verdict {
    pub(crate) overall: String,
    pub(crate) rationale: Vec<String>,
    pub(crate) blocking_failures: Vec<String>,
    pub(crate) improved: Vec<String>,
    pub(crate) neutral: Vec<String>,
    pub(crate) worse: Vec<String>,
    pub(crate) uncertain: Vec<String>,
}

pub(crate) fn machine_summary(spec: &MachineSpec) -> MachineSummary {
    MachineSummary {
        name: spec.name.clone(),
        cpus: spec.cpus,
        devices: spec
            .devices
            .iter()
            .map(|device| {
                format!(
                    "{}:{} class={} modalias={}",
                    device.bus, device.id, device.class, device.modalias
                )
            })
            .collect(),
        sata_hosts: spec
            .sata
            .iter()
            .map(|host| format!("{} behind {}", host.host, host.controller))
            .collect(),
        backlight: format!("{} max={}", spec.backlight_device, spec.backlight_max),
        note: "Every device here is modelled. None of it describes a physical machine.".to_string(),
    }
}

pub(crate) fn scenario_summary(scenario: &Scenario) -> ScenarioSummary {
    ScenarioSummary {
        id: scenario.id.clone(),
        description: scenario.description.clone(),
        safety_only: scenario.safety_only,
        cycles: scenario.cycles,
        step_seconds: scenario.step_seconds,
        workload: scenario.workload.id.clone(),
        starts_on_ac: scenario.env.on_ac,
        faults: scenario.faults.iter().map(describe_fault).collect(),
    }
}

fn describe_fault(fault: &SimFault) -> String {
    match fault {
        SimFault::WriteDenied { path, at_cycle } => {
            format!("writes to {path} are refused from cycle {at_cycle}")
        }
        SimFault::ShortWrite { path, at_cycle } => {
            format!("the write to {path} is truncated at cycle {at_cycle}")
        }
        SimFault::ExternalDrift {
            path,
            at_cycle,
            value,
        } => {
            format!("a third party sets {path} to {value} at cycle {at_cycle}")
        }
        SimFault::SensorMissing { path, at_cycle } => {
            format!("{path} disappears at cycle {at_cycle}")
        }
        SimFault::SensorMalformed { path, at_cycle, .. } => {
            format!("{path} becomes unparseable at cycle {at_cycle}")
        }
        SimFault::Crash { after_cycle } => {
            format!("the daemon dies without restoring after cycle {after_cycle}")
        }
        SimFault::RestoreDenied { path } => {
            format!("restoration of {path} is refused at shutdown")
        }
    }
}

/// Build one receipt per control optid attempted to change, from the simulated
/// machine's own record.
pub(crate) fn build_receipts(
    machine: &SimMachine,
    baseline: &BTreeMap<String, String>,
    post: &BTreeMap<String, String>,
    writes: &[WriteRecord],
) -> Vec<Receipt> {
    let domains = machine.control_domains();
    let drifted = machine.drifted_controls();

    let mut grouped: BTreeMap<String, Vec<&WriteRecord>> = BTreeMap::new();
    for record in writes {
        if let Some(id) = &record.control_id {
            grouped.entry(id.clone()).or_default().push(record);
        }
    }

    let mut receipts = Vec::new();
    for (control_id, records) in grouped {
        let domain = domains
            .get(&control_id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let first = records[0];
        let baseline_value = baseline.get(&control_id).cloned().unwrap_or_default();
        // The action under test is the write that moved the control away from
        // the value it powered on with. Anything after it is optid changing its
        // own mind or handing the control back, and reporting one of those as
        // "the request" would hide the action that actually happened.
        let live: Vec<&WriteRecord> = records
            .iter()
            .copied()
            .filter(|record| record.phase != super::machine::Phase::Shutdown)
            .collect();
        let last_request = live
            .iter()
            .find(|record| {
                record.requested_value != baseline_value
                    && matches!(record.result, WriteResult::Applied)
            })
            .or_else(|| {
                live.iter()
                    .find(|record| record.requested_value != baseline_value)
            })
            .or_else(|| live.last())
            .copied()
            .unwrap_or(first);
        let previous_value = last_request
            .previous_value
            .clone()
            .or_else(|| first.previous_value.clone())
            .unwrap_or_default();
        let requested_value = last_request.requested_value.clone();
        let read_back_value = last_request
            .read_back_value
            .clone()
            .unwrap_or_else(|| "<unreadable>".to_string());
        let restored_value = post.get(&control_id).cloned().unwrap_or_default();
        let became_active = read_back_value == requested_value
            && matches!(last_request.result, WriteResult::Applied);
        let restored_correctly = restored_value == baseline_value;
        let classification = if previous_value.is_empty() && baseline_value.is_empty() {
            ReceiptClass::IncompleteReceipt
        } else if drifted.contains(&control_id) {
            ReceiptClass::ExternallyDrifted
        } else if records
            .iter()
            .any(|record| matches!(record.result, WriteResult::Inert))
            && !became_active
        {
            ReceiptClass::InertControl
        } else if records
            .iter()
            .all(|record| matches!(record.result, WriteResult::Rejected { .. }))
        {
            ReceiptClass::WriteRefused
        } else if !became_active {
            ReceiptClass::ReadBackMismatch
        } else if restored_correctly {
            ReceiptClass::ActiveAndRestored
        } else {
            ReceiptClass::ActiveNotRestored
        };
        receipts.push(Receipt {
            control_id,
            domain,
            previous_value: if previous_value.is_empty() {
                baseline_value
            } else {
                previous_value
            },
            requested_value,
            read_back_value,
            restored_value,
            became_active,
            restored_correctly,
            write_attempts: records.len() as u32,
            classification,
        });
    }
    receipts
}

/// Read optid's own account of what it did.
pub(crate) fn read_envelope(state_dir: &Path) -> EnvelopeSummary {
    let mut summary = EnvelopeSummary::default();
    let path = state_dir.join("control-cycles.jsonl");
    let Ok(text) = fs::read_to_string(&path) else {
        // Deliberately path-free: this string is part of the determinism
        // digest, and a run-specific path would make every run look unique.
        summary
            .parse_errors
            .push("no control-cycle envelope".to_string());
        return summary;
    };
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                summary
                    .parse_errors
                    .push(format!("cycle line {index}: {error}"));
                continue;
            }
        };
        summary.cycles += 1;
        if let Some(version) = value.get("schema_version").and_then(|v| v.as_u64()) {
            summary.schema_versions.insert(version as u32);
        }
        if let Some(domains) = value.get("domains").and_then(|v| v.as_array()) {
            for domain in domains {
                let name = domain
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                summary.domains_seen.insert(name.clone());
                if let Some(support) = domain.get("support").and_then(|v| v.as_str()) {
                    summary
                        .domain_support
                        .insert(name.clone(), support.to_string());
                }
                if let Some(mode) = domain.get("selected_mode").and_then(|v| v.as_str()) {
                    summary.domain_modes.insert(name.clone(), mode.to_string());
                }
                let Some(outcomes) = domain.get("action_outcomes").and_then(|v| v.as_array())
                else {
                    continue;
                };
                for outcome in outcomes {
                    if let Some(gates) = outcome.get("gates").and_then(|v| v.as_array()) {
                        for gate in gates {
                            let disposition = gate
                                .get("disposition")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            if disposition == "denied" {
                                let reason = gate
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("denied")
                                    .to_string();
                                summary.gate_denials.insert(name.clone(), reason);
                            }
                        }
                    }
                    let Some(targets) = outcome.get("targets").and_then(|v| v.as_array()) else {
                        continue;
                    };
                    for target in targets {
                        let target_id = target
                            .get("target_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let reason = target
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        if target.get("write_attempted").and_then(|v| v.as_bool()) == Some(true) {
                            summary.targets_written.insert(target_id.clone());
                        }
                        summary.target_reasons.insert(target_id, reason);
                    }
                }
            }
        }
        if let Some(restores) = value.get("restore_outcomes").and_then(|v| v.as_array()) {
            for restore in restores {
                let target_id = restore
                    .get("target_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let reason = restore
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                summary.restore_targets.insert(target_id, reason);
            }
        }
    }
    summary
}

pub(crate) fn read_circuit_state(state_dir: &Path) -> String {
    let Ok(text) = fs::read_to_string(state_dir.join("circuits.json")) else {
        return "absent".to_string();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return "unparseable".to_string();
    };
    let global = value
        .get("global_open")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let open_scopes = value
        .get("scopes")
        .and_then(|v| v.as_array())
        .map(|scopes| {
            scopes
                .iter()
                .filter(|scope| {
                    scope.get("state").and_then(|v| v.as_str()) == Some("open")
                        || scope.get("open").and_then(|v| v.as_bool()) == Some(true)
                })
                .count()
        })
        .unwrap_or(0);
    format!("global_open={global} open_scopes={open_scopes}")
}

/// Aggregate the per-cycle model output into the reported metric set.
pub(crate) fn aggregate(samples: &[StepSample]) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    if samples.is_empty() {
        return out;
    }
    let count = samples.len() as f64;
    let mut sum_p99 = 0.0;
    let mut sum_mean = 0.0;
    let mut sum_throughput = 0.0;
    let mut sum_work = 0.0;
    let mut sum_cpu_stall = 0.0;
    let mut sum_mem_stall = 0.0;
    let mut sum_io_stall = 0.0;
    let mut sum_energy = 0.0;
    let mut sum_power = 0.0;
    let mut sum_temp = 0.0;
    let mut peak_temp = f64::MIN;
    let mut iterations = 0u64;
    for sample in samples {
        sum_p99 += sample.metrics.foreground_p99_latency_us;
        sum_mean += sample.metrics.foreground_mean_latency_us;
        sum_throughput += sample.metrics.throughput_ops_per_s;
        sum_work += sample.metrics.completed_work_units;
        sum_cpu_stall += sample.metrics.cpu_stall_pct;
        sum_mem_stall += sample.metrics.memory_stall_pct;
        sum_io_stall += sample.metrics.io_stall_pct;
        sum_energy += sample.metrics.energy_j;
        sum_power += sample.metrics.mean_power_w;
        sum_temp += sample.metrics.die_temp_end_c;
        if sample.metrics.die_temp_end_c > peak_temp {
            peak_temp = sample.metrics.die_temp_end_c;
        }
        iterations += sample.metrics.iterations;
    }
    out.insert("foreground_p99_latency_us".to_string(), sum_p99 / count);
    out.insert("foreground_mean_latency_us".to_string(), sum_mean / count);
    out.insert("throughput_ops_per_s".to_string(), sum_throughput / count);
    out.insert("completed_work_units".to_string(), sum_work);
    out.insert("cpu_stall_pct".to_string(), sum_cpu_stall / count);
    out.insert("memory_stall_pct".to_string(), sum_mem_stall / count);
    out.insert("io_stall_pct".to_string(), sum_io_stall / count);
    out.insert("energy_j".to_string(), sum_energy);
    out.insert("mean_power_w".to_string(), sum_power / count);
    out.insert("mean_die_temp_c".to_string(), sum_temp / count);
    out.insert("peak_die_temp_c".to_string(), peak_temp);
    out.insert("iterations".to_string(), iterations as f64);
    out.insert("cycles".to_string(), count);
    out
}

/// Re-evaluate the recorded machine trajectory under every assumption set.
///
/// This bounds *model-parameter* uncertainty. It does not re-run the closed
/// loop, so it does not bound the uncertainty in optid's own decision
/// trajectory; that limitation is stated in the report.
pub(crate) fn sensitivity(
    samples: &[StepSample],
    scenario: &Scenario,
) -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut out = BTreeMap::new();
    for assumptions in Assumptions::grid() {
        let mut recomputed = Vec::new();
        for sample in samples {
            let metrics = evaluate_step(
                &sample.active,
                &sample.env,
                &scenario.workload,
                &assumptions,
                scenario.step_seconds as f64,
            );
            recomputed.push(StepSample {
                cycle: sample.cycle,
                env: sample.env.clone(),
                active: sample.active.clone(),
                metrics,
            });
        }
        out.insert(assumptions.id.clone(), aggregate(&recomputed));
    }
    out
}

/// Reject a result that cannot support a claim.
pub(crate) fn validate_metrics(
    aggregate: &BTreeMap<String, f64>,
    samples: &[StepSample],
) -> Vec<String> {
    let mut rejections = Vec::new();
    if samples.is_empty() {
        rejections.push("no modelled cycles were produced".to_string());
        return rejections;
    }
    for metric in metric_names() {
        let Some(value) = aggregate.get(metric) else {
            rejections.push(format!("missing metric {metric}"));
            continue;
        };
        if value.is_nan() {
            rejections.push(format!("{metric} is NaN"));
        }
        if value.is_infinite() {
            rejections.push(format!("{metric} is infinite"));
        }
        if *value < 0.0 {
            rejections.push(format!("{metric} is negative ({value})"));
        }
    }
    if aggregate.get("iterations").copied().unwrap_or(0.0) <= 0.0 {
        rejections.push("zero iterations".to_string());
    }
    for latency in ["foreground_p99_latency_us", "foreground_mean_latency_us"] {
        if aggregate.get(latency).copied().unwrap_or(0.0) <= 0.0 {
            rejections.push(format!(
                "{latency} is zero or negative, which is impossible"
            ));
        }
    }
    if aggregate
        .get("throughput_ops_per_s")
        .copied()
        .unwrap_or(0.0)
        <= 0.0
    {
        rejections.push("throughput is zero or negative".to_string());
    }
    if aggregate.get("mean_power_w").copied().unwrap_or(0.0) <= 0.0 {
        rejections.push("modelled power is zero or negative".to_string());
    }
    let peak = aggregate.get("peak_die_temp_c").copied().unwrap_or(0.0);
    if !(0.0..=125.0).contains(&peak) {
        rejections.push(format!(
            "modelled peak die temperature {peak} is impossible"
        ));
    }
    let p99 = aggregate
        .get("foreground_p99_latency_us")
        .copied()
        .unwrap_or(0.0);
    let mean = aggregate
        .get("foreground_mean_latency_us")
        .copied()
        .unwrap_or(0.0);
    if p99 < mean {
        rejections.push("p99 latency below mean latency is impossible".to_string());
    }
    rejections
}

/// Reject an incomplete receipt or a receipt optid's own envelope contradicts.
pub(crate) fn validate_receipts(receipts: &[Receipt], envelope: &EnvelopeSummary) -> Vec<String> {
    let mut rejections = Vec::new();
    for receipt in receipts {
        if receipt.requested_value.is_empty() {
            rejections.push(format!(
                "{}: receipt has no requested value",
                receipt.control_id
            ));
        }
        if receipt.previous_value.is_empty() {
            rejections.push(format!(
                "{}: receipt has no previous value",
                receipt.control_id
            ));
        }
        if receipt.read_back_value == "<unreadable>" {
            rejections.push(format!(
                "{}: receipt has no read-back value",
                receipt.control_id
            ));
        }
        if receipt.classification == ReceiptClass::ActiveNotRestored {
            rejections.push(format!(
                "{}: became active and did not restore (previous={}, ended at {})",
                receipt.control_id, receipt.previous_value, receipt.restored_value
            ));
        }
        if receipt.classification == ReceiptClass::IncompleteReceipt {
            rejections.push(format!("{}: incomplete receipt", receipt.control_id));
        }
    }
    if !receipts.is_empty() && envelope.cycles == 0 && envelope.parse_errors.is_empty() {
        rejections
            .push("controls were written but optid recorded no control cycle at all".to_string());
    }
    for error in &envelope.parse_errors {
        if !error.starts_with("no control-cycle envelope") {
            rejections.push(format!("envelope: {error}"));
        }
    }
    rejections
}

/// Replace the reconciler's per-process transaction generation identifier with
/// a placeholder. The generation is `SystemTime::now()` in nanoseconds plus the
/// PID, so it differs between two runs that behaved identically. Only the
/// identifier is normalised; the behaviour it appears in is still compared.
fn normalise_generations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for character in text.chars() {
        if character.is_ascii_hexdigit() {
            run.push(character);
            continue;
        }
        if run.len() >= 32 {
            out.push_str("<generation>");
        } else {
            out.push_str(&run);
        }
        run.clear();
        out.push(character);
    }
    if run.len() >= 32 {
        out.push_str("<generation>");
    } else {
        out.push_str(&run);
    }
    out
}

fn digest(trial: &Trial) -> String {
    let value = serde_json::json!({
        "daemon_outcome": trial.daemon_outcome,
        "recovery_outcome": trial.recovery_outcome,
        "s3d_recovery": trial.s3d_recovery,
        "cycles_completed": trial.cycles_completed,
        "host_write_attempts": trial.host_write_attempts,
        "writes": trial.writes,
        "samples": trial.samples,
        "receipts": trial.receipts,
        "envelope": trial.envelope,
        "aggregate": trial.aggregate,
        "sensitivity": trial.sensitivity,
        "rejections": trial.rejections,
        "circuit_state": trial.circuit_state,
    });
    normalise_generations(&serde_json::to_string(&value).unwrap_or_default())
}

pub(crate) fn determinism(trials: &[Trial], repeats: u32) -> DeterminismReport {
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for trial in trials {
        groups
            .entry((trial.arm.clone(), trial.scenario.clone()))
            .or_default()
            .push(digest(trial));
    }
    let mut divergent = Vec::new();
    let mut identical = 0usize;
    for ((arm, scenario), digests) in &groups {
        if digests.windows(2).all(|pair| pair[0] == pair[1]) {
            identical += 1;
        } else {
            divergent.push(format!("{arm} / {scenario}"));
        }
    }
    DeterminismReport {
        repeats,
        compared_groups: groups.len(),
        identical_groups: identical,
        divergent,
    }
}

fn first_trial<'a>(trials: &'a [Trial], arm: &str, scenario: &str) -> Option<&'a Trial> {
    trials
        .iter()
        .find(|trial| trial.arm == arm && trial.scenario == scenario && trial.repeat == 0)
}

/// The smallest absolute change in a metric that is worth calling a change.
///
/// Without this a metric whose baseline is near zero produces enormous relative
/// deltas from a physically trivial difference — a 0.02-percentage-point stall
/// against a 0.003 baseline reads as "600% worse". The floor is stated in the
/// metric's own units and is part of the model's assumptions.
fn metric_floor(metric: &str) -> f64 {
    match metric {
        "foreground_p99_latency_us" | "foreground_mean_latency_us" => 10.0,
        "cpu_stall_pct" | "memory_stall_pct" | "io_stall_pct" => 1.0,
        "energy_j" => 1.0,
        "mean_power_w" => 0.05,
        "peak_die_temp_c" | "mean_die_temp_c" => 0.5,
        "throughput_ops_per_s" => 1.0,
        "completed_work_units" => 0.01,
        _ => 0.0,
    }
}

/// Express a change as "better is positive", relative to the baseline but never
/// relative to a number smaller than the metric's noise floor.
fn relative_delta(metric: &str, baseline: f64, candidate: f64) -> Option<f64> {
    if !baseline.is_finite() || !candidate.is_finite() {
        return None;
    }
    let floor = metric_floor(metric);
    let difference = candidate - baseline;
    if difference.abs() < floor {
        return Some(0.0);
    }
    let denominator = if baseline.abs() > floor {
        baseline.abs()
    } else {
        floor
    };
    let raw = difference / denominator;
    match lower_is_better(metric) {
        Some(true) => Some(-raw),
        Some(false) => Some(raw),
        None => Some(raw),
    }
}

fn direction_of(delta: f64) -> Direction {
    if delta > MEANINGFUL_RELATIVE_DELTA {
        Direction::Improved
    } else if delta < -MEANINGFUL_RELATIVE_DELTA {
        Direction::Worse
    } else {
        Direction::NoMeaningfulDifference
    }
}

/// One metric compared between two arms, before it is turned into a row.
struct Judgement {
    delta: f64,
    direction: Direction,
    stable: bool,
    range: (f64, f64),
    note: String,
}

fn compare_one(metric: &str, baseline: &Trial, candidate: &Trial) -> Option<Judgement> {
    let base_value = *baseline.aggregate.get(metric)?;
    let arm_value = *candidate.aggregate.get(metric)?;
    let delta = relative_delta(metric, base_value, arm_value)?;
    let direction = direction_of(delta);
    let mut low = delta;
    let mut high = delta;
    let mut stable = true;
    for (assumption_id, metrics) in &candidate.sensitivity {
        let Some(base_metrics) = baseline.sensitivity.get(assumption_id) else {
            continue;
        };
        let (Some(base), Some(arm)) = (base_metrics.get(metric), metrics.get(metric)) else {
            continue;
        };
        let Some(alt) = relative_delta(metric, *base, *arm) else {
            continue;
        };
        if alt < low {
            low = alt;
        }
        if alt > high {
            high = alt;
        }
        if direction_of(alt) != direction {
            stable = false;
        }
    }
    let note = if stable {
        format!(
            "direction holds across all {} assumption sets",
            candidate.sensitivity.len()
        )
    } else {
        "at least one reasonable assumption set reverses or neutralises this result".to_string()
    };
    Some(Judgement {
        delta,
        direction,
        stable,
        range: (low, high),
        note,
    })
}

pub(crate) fn compare(trials: &[Trial], arms: &[Arm], scenarios: &[Scenario]) -> Vec<Comparison> {
    let mut out = Vec::new();
    for scenario in scenarios {
        if scenario.safety_only {
            continue;
        }
        let Some(baseline) = first_trial(trials, BASELINE_ARM, &scenario.id) else {
            continue;
        };
        for arm in arms {
            if arm.id == BASELINE_ARM {
                continue;
            }
            let Some(candidate) = first_trial(trials, &arm.id, &scenario.id) else {
                continue;
            };
            for metric in metric_names() {
                let Some(judgement) = compare_one(metric, baseline, candidate) else {
                    out.push(Comparison {
                        arm: arm.id.clone(),
                        scenario: scenario.id.clone(),
                        metric: metric.to_string(),
                        baseline_value: f64::NAN,
                        arm_value: f64::NAN,
                        relative_delta: 0.0,
                        absolute_delta: 0.0,
                        baseline_below_floor: false,
                        direction: Direction::Unsupported,
                        direction_stable: false,
                        sensitivity_range: (0.0, 0.0),
                        note: "metric unavailable in one arm; not judged".to_string(),
                    });
                    continue;
                };
                let unsupported =
                    !candidate.rejections.is_empty() || !baseline.rejections.is_empty();
                let base_value = *baseline.aggregate.get(metric).unwrap_or(&f64::NAN);
                let arm_value = *candidate.aggregate.get(metric).unwrap_or(&f64::NAN);
                out.push(Comparison {
                    arm: arm.id.clone(),
                    scenario: scenario.id.clone(),
                    metric: metric.to_string(),
                    baseline_value: base_value,
                    arm_value,
                    relative_delta: judgement.delta,
                    absolute_delta: arm_value - base_value,
                    baseline_below_floor: base_value.abs() <= metric_floor(metric),
                    direction: if unsupported {
                        Direction::Unsupported
                    } else {
                        judgement.direction
                    },
                    direction_stable: judgement.stable,
                    sensitivity_range: judgement.range,
                    note: if unsupported {
                        "a rejected result in this pair makes the comparison unusable".to_string()
                    } else {
                        judgement.note
                    },
                });
            }
        }
    }
    out
}

/// Which optid actions caused each change, from the isolation arms.
pub(crate) fn attribute(
    trials: &[Trial],
    arms: &[Arm],
    scenarios: &[Scenario],
) -> Vec<Attribution> {
    let mut out = Vec::new();
    for scenario in scenarios {
        if scenario.safety_only {
            continue;
        }
        let Some(baseline) = first_trial(trials, BASELINE_ARM, &scenario.id) else {
            continue;
        };
        for domain in KERNEL_DOMAINS {
            let arm_id = format!("only_{domain}");
            if !arms.iter().any(|arm| arm.id == arm_id) {
                continue;
            }
            let Some(candidate) = first_trial(trials, &arm_id, &scenario.id) else {
                continue;
            };
            let actions: Vec<String> = candidate
                .receipts
                .iter()
                .filter(|receipt| receipt.domain == domain)
                .map(|receipt| {
                    format!(
                        "{}: {} -> {} (read back {}, restored {})",
                        receipt.control_id,
                        receipt.previous_value,
                        receipt.requested_value,
                        receipt.read_back_value,
                        receipt.restored_value
                    )
                })
                .collect();
            for metric in metric_names() {
                let Some(judgement) = compare_one(metric, baseline, candidate) else {
                    continue;
                };
                if judgement.direction == Direction::NoMeaningfulDifference {
                    continue;
                }
                out.push(Attribution {
                    scenario: scenario.id.clone(),
                    metric: metric.to_string(),
                    domain: domain.to_string(),
                    relative_delta: judgement.delta,
                    direction: judgement.direction,
                    direction_stable: judgement.stable,
                    actions: actions.clone(),
                });
            }
        }
    }
    out
}

/// Did the combined configuration hide an individually harmful action?
pub(crate) fn masking(
    comparisons: &[Comparison],
    attribution: &[Attribution],
) -> Vec<MaskingFinding> {
    let mut out = Vec::new();
    for item in attribution {
        if item.direction != Direction::Worse {
            continue;
        }
        let Some(combined) = comparisons.iter().find(|comparison| {
            comparison.arm == "full_enabled"
                && comparison.scenario == item.scenario
                && comparison.metric == item.metric
        }) else {
            continue;
        };
        if combined.direction != Direction::Worse {
            out.push(MaskingFinding {
                scenario: item.scenario.clone(),
                metric: item.metric.clone(),
                domain: item.domain.clone(),
                isolated_relative_delta: item.relative_delta,
                combined_relative_delta: combined.relative_delta,
                detail: format!(
                    "{} alone makes {} worse by {:.1}%, but the fully enabled configuration reports \
                     {:?} ({:+.1}%). The combined result hides this action.",
                    item.domain,
                    item.metric,
                    -item.relative_delta * 100.0,
                    combined.direction,
                    combined.relative_delta * 100.0
                ),
            });
        }
    }
    out
}

pub(crate) fn controls(
    trials: &[Trial],
    comparisons: &[Comparison],
    scenarios: &[Scenario],
) -> ControlReport {
    let no_change_arms = ["off_all_domains", "full_observe"];
    // The control proves the result system can see "nothing changed". It is
    // evaluated on scenarios with no injected fault and no policy reload; the
    // reload case is reported separately as a finding, because there optid
    // genuinely does change state and the control would mask that.
    let clean: BTreeSet<&str> =
        scenarios
            .iter()
            .filter(|scenario| {
                scenario.faults.is_empty()
                    && scenario.events.values().flatten().all(|event| {
                        !matches!(event, super::scenarios::StepEvent::ReloadPolicy { .. })
                    })
            })
            .map(|scenario| scenario.id.as_str())
            .collect();
    let mut violations = Vec::new();
    for arm in no_change_arms {
        for trial in trials
            .iter()
            .filter(|trial| trial.arm == arm && clean.contains(trial.scenario.as_str()))
        {
            for receipt in &trial.receipts {
                if receipt.became_active {
                    violations.push(format!(
                        "{arm} / {}: {} changed to {} although no domain may actuate",
                        trial.scenario, receipt.control_id, receipt.read_back_value
                    ));
                }
            }
            if trial.host_write_attempts > 0 {
                violations.push(format!(
                    "{arm} / {}: {} write attempts left the simulated machine",
                    trial.scenario, trial.host_write_attempts
                ));
            }
        }
    }
    let harmful_evidence: Vec<String> = comparisons
        .iter()
        .filter(|comparison| {
            comparison.arm == "harmful_control" && comparison.direction == Direction::Worse
        })
        .map(|comparison| {
            format!(
                "{} / {}: {:.1}% worse",
                comparison.scenario,
                comparison.metric,
                -comparison.relative_delta * 100.0
            )
        })
        .collect();
    ControlReport {
        no_change_control_arms: no_change_arms.iter().map(|arm| arm.to_string()).collect(),
        no_change_control_held: violations.is_empty(),
        no_change_violations: violations,
        harmful_control_arm: "harmful_control".to_string(),
        harmful_control_detected: !harmful_evidence.is_empty(),
        harmful_control_evidence: harmful_evidence,
    }
}

pub(crate) fn safety(trials: &[Trial], scenarios: &[Scenario]) -> SafetyReport {
    let mut total = 0usize;
    let mut active = 0usize;
    let mut restored = 0usize;
    let mut unrestored_clean = Vec::new();
    let mut unrestored_faulted = Vec::new();
    let faulted: BTreeSet<&str> = scenarios
        .iter()
        .filter(|scenario| !scenario.faults.is_empty())
        .map(|scenario| scenario.id.as_str())
        .collect();
    let mut inert = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut circuit_opened = Vec::new();
    let mut containment_clean = true;
    let mut notes = Vec::new();

    for trial in trials {
        if trial.repeat != 0 {
            continue;
        }
        for receipt in &trial.receipts {
            total += 1;
            if receipt.became_active {
                active += 1;
                if receipt.restored_correctly
                    || receipt.classification == ReceiptClass::ExternallyDrifted
                {
                    restored += 1;
                } else {
                    let entry = format!(
                        "{} / {}: {} ended at {} instead of {}",
                        trial.arm,
                        trial.scenario,
                        receipt.control_id,
                        receipt.restored_value,
                        receipt.previous_value
                    );
                    if faulted.contains(trial.scenario.as_str()) {
                        unrestored_faulted.push(entry);
                    } else {
                        unrestored_clean.push(entry);
                    }
                }
            }
            if receipt.classification == ReceiptClass::InertControl {
                inert.insert(receipt.control_id.clone());
            }
            if receipt.classification == ReceiptClass::WriteRefused {
                refused.insert(receipt.control_id.clone());
            }
        }
        if trial.circuit_state.contains("global_open=true")
            || trial.circuit_state.contains("open_scopes=1")
        {
            circuit_opened.push(format!("{} / {}", trial.arm, trial.scenario));
        }
        if trial.host_write_attempts > 0 || !trial.containment_violations.is_empty() {
            containment_clean = false;
        }
    }

    let crash_scenarios: Vec<&Scenario> = scenarios
        .iter()
        .filter(|scenario| {
            scenario
                .faults
                .iter()
                .any(|fault| matches!(fault, SimFault::Crash { .. }))
        })
        .collect();
    let crash_trials: Vec<&Trial> = trials
        .iter()
        .filter(|trial| {
            trial.repeat == 0
                && crash_scenarios
                    .iter()
                    .any(|scenario| scenario.id == trial.scenario)
                && trial.recovery_outcome.is_some()
        })
        .collect();
    let crash_recovery_ran = !crash_trials.is_empty();
    let crash_recovery_restored_everything = crash_trials.iter().all(|trial| {
        trial
            .receipts
            .iter()
            .filter(|receipt| receipt.became_active)
            .all(|receipt| receipt.restored_correctly)
    });
    let failed_restoration_detected = trials.iter().any(|trial| {
        trial.scenario == "failed_restoration"
            && trial
                .receipts
                .iter()
                .any(|receipt| receipt.classification == ReceiptClass::ActiveNotRestored)
    });
    if crash_recovery_ran {
        notes.push(
            "Crash recovery was exercised through the production daemon restart path \
             (reconciler hydrate, journal replay and handback), after clearing the tmpfs state \
             directory to simulate a reboot."
                .to_string(),
        );
    }
    notes.push(
        "The standalone S3D `optid-recover` executable was run as a real subprocess before every \
         supervised restart, matching `optid-apply.service`'s `Requires=optid-recover.service`. \
         Its `--machine-root` flag, which exists only in a `test-simulation` build, rebases every \
         recorded target path into the simulated machine, so no recovery write can reach a host \
         path."
            .to_string(),
    );
    notes.push(format!(
        "The `{SYSTEMD_DOMAIN}` domain is held off in every arm: actuating it means executing \
         `systemctl` against real system services, which this harness must never do. It is \
         reported as unsupported in simulation, not as a passing test."
    ));

    SafetyReport {
        total_receipts: total,
        active_receipts: active,
        restored_receipts: restored,
        unrestored_without_a_fault: unrestored_clean,
        unrestored_under_an_injected_fault: unrestored_faulted,
        inert_controls: inert.into_iter().collect(),
        refused_writes: refused.into_iter().collect(),
        crash_recovery_ran,
        crash_recovery_restored_everything,
        circuit_opened_in: circuit_opened,
        failed_restoration_detected,
        containment_clean,
        notes,
    }
}

pub(crate) fn support(trials: &[Trial], scenarios: &[Scenario]) -> SupportReport {
    let clean: BTreeSet<&str> =
        scenarios
            .iter()
            .filter(|scenario| {
                scenario.faults.is_empty()
                    && scenario.events.values().flatten().all(|event| {
                        !matches!(event, super::scenarios::StepEvent::ReloadPolicy { .. })
                    })
            })
            .map(|scenario| scenario.id.as_str())
            .collect();
    let mut supported = BTreeSet::new();
    let mut unsupported: BTreeMap<String, String> = BTreeMap::new();
    let mut rejected = Vec::new();

    let mut by_arm: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut escalated: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for trial in trials.iter().filter(|trial| trial.repeat == 0) {
        let is_clean = clean.contains(trial.scenario.as_str());
        let is_off_arm = trial.arm == "off_all_domains" || trial.arm == "full_observe";
        for receipt in &trial.receipts {
            if receipt.became_active {
                supported.insert(receipt.domain.clone());
                if is_clean {
                    by_arm
                        .entry(trial.arm.clone())
                        .or_default()
                        .insert(receipt.domain.clone());
                } else if is_off_arm {
                    escalated
                        .entry(trial.arm.clone())
                        .or_default()
                        .insert(receipt.domain.clone());
                }
            }
        }
        for rejection in &trial.rejections {
            rejected.push(format!("{} / {}: {rejection}", trial.arm, trial.scenario));
        }
    }
    let active_domains_by_arm: BTreeMap<String, Vec<String>> = by_arm
        .into_iter()
        .map(|(arm, domains)| (arm, domains.into_iter().collect()))
        .collect();
    for domain in KERNEL_DOMAINS {
        if !supported.contains(domain) {
            let reason = trials
                .iter()
                .filter(|trial| trial.arm == "full_enabled" && trial.repeat == 0)
                .find_map(|trial| trial.envelope.gate_denials.get(domain).cloned())
                .unwrap_or_else(|| {
                    "no action for this domain ever became active on the simulated machine"
                        .to_string()
                });
            unsupported.insert(domain.to_string(), reason);
        }
    }
    unsupported.insert(
        SYSTEMD_DOMAIN.to_string(),
        "held off in every arm: actuating it would execute `systemctl` against real system services"
            .to_string(),
    );
    rejected.sort();
    rejected.dedup();
    SupportReport {
        supported_domains: supported.into_iter().collect(),
        active_domains_by_arm,
        escalated_domains_by_arm: escalated
            .into_iter()
            .map(|(arm, domains)| (arm, domains.into_iter().collect()))
            .collect(),
        unsupported_domains: unsupported,
        rejected_results: rejected,
    }
}

/// Behaviours the matrix surfaced that the comparison table alone would not
/// show. These are statements about the simulated run, not about hardware.
pub(crate) fn findings(trials: &[Trial], scenarios: &[Scenario]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // 1. A policy file that becomes unparseable at runtime falls back to the
    //    curated baseline, whose per-domain default is `actuate`. `apply_armed`
    //    is computed once at startup and is not re-evaluated, so an operator's
    //    explicit `off` configuration is replaced by actuation mid-run.
    let reload_scenarios: Vec<&str> = scenarios
        .iter()
        .filter(|scenario| {
            scenario.events.values().flatten().any(|event| {
                matches!(
                    event,
                    super::scenarios::StepEvent::ReloadPolicy { valid: false }
                )
            })
        })
        .map(|scenario| scenario.id.as_str())
        .collect();
    let mut escalation_evidence = Vec::new();
    for trial in trials.iter().filter(|trial| trial.repeat == 0) {
        if !reload_scenarios.contains(&trial.scenario.as_str()) {
            continue;
        }
        if trial.arm != "off_all_domains" && trial.arm != "full_observe" {
            continue;
        }
        for receipt in trial
            .receipts
            .iter()
            .filter(|receipt| receipt.became_active)
        {
            escalation_evidence.push(format!(
                "{} / {}: {} moved {} -> {} even though every domain is configured {}",
                trial.arm,
                trial.scenario,
                receipt.control_id,
                receipt.previous_value,
                receipt.read_back_value,
                if trial.arm == "off_all_domains" {
                    "off"
                } else {
                    "observe"
                }
            ));
        }
    }
    if !escalation_evidence.is_empty() {
        escalation_evidence.truncate(12);
        findings.push(Finding {
            id: "policy_reload_fallback_escalates_domain_modes".to_string(),
            severity: "high".to_string(),
            summary: concat!(
                "A policy file that becomes unparseable while optid is running is replaced by ",
                "`Policy::curated_baseline()`, whose per-domain default is `actuate`. The run ",
                "loop reloads the policy on every cycle but computes `apply_armed` and the load ",
                "state only at startup, so an operator's explicit `mode = \"off\"` (or ",
                "`observe`) configuration silently becomes actuation mid-run.",
            )
            .to_string(),
            evidence: escalation_evidence,
        });
    }

    // 2. Hot-removing a device optid owns aborts the control loop before the
    //    shutdown handback runs; only a supervisor restart hands the machine
    //    back.
    let mut hotplug_evidence = Vec::new();
    for trial in trials.iter().filter(|trial| trial.repeat == 0) {
        if trial.daemon_outcome.starts_with("error:")
            && trial.daemon_outcome.contains("canonicalize")
        {
            hotplug_evidence.push(format!(
                "{} / {}: {} (recovery: {})",
                trial.arm,
                trial.scenario,
                trial.daemon_outcome,
                trial
                    .recovery_outcome
                    .clone()
                    .unwrap_or_else(|| "none".to_string())
            ));
        }
    }
    if !hotplug_evidence.is_empty() {
        hotplug_evidence.truncate(8);
        findings.push(Finding {
            id: "owned_target_hot_removal_aborts_the_control_loop".to_string(),
            severity: "medium".to_string(),
            summary: concat!(
                "Removing a device optid owns makes the reconciler's transaction target ",
                "canonicalisation fail, and the error propagates out of the control loop. The ",
                "loop exits before its shutdown handback, so every owned target stays applied. ",
                "The first supervised restart then refuses to start at all (StaleGeneration on ",
                "the vanished target's record); only the S3D `optid-recover` pass that runs ",
                "before the next restart clears it. Two restarts and one recovery pass are ",
                "needed to hand the machine back, and `optid-apply.service` allows three starts ",
                "per minute.",
            )
            .to_string(),
            evidence: hotplug_evidence,
        });
    }

    // 3. Controls the machine exposes that no arm ever attempted.
    let mut never: BTreeSet<String> = BTreeSet::new();
    if let Some(full) = trials
        .iter()
        .find(|trial| trial.arm == "full_enabled" && trial.repeat == 0)
    {
        for control in &full.untouched_controls {
            never.insert(control.clone());
        }
    }
    for trial in trials
        .iter()
        .filter(|trial| trial.arm == "full_enabled" && trial.repeat == 0)
    {
        never.retain(|control| trial.untouched_controls.contains(control));
    }
    if !never.is_empty() {
        findings.push(Finding {
            id: "controls_never_attempted_by_the_fully_enabled_arm".to_string(),
            severity: "informational".to_string(),
            summary: concat!(
                "The simulated machine exposes these controls and the fully enabled ",
                "configuration never attempted to write any of them in any scenario.",
            )
            .to_string(),
            evidence: never.into_iter().collect(),
        });
    }

    findings
}

pub(crate) fn verdict(
    comparisons: &[Comparison],
    controls: &ControlReport,
    safety: &SafetyReport,
    determinism: &DeterminismReport,
    support: &SupportReport,
) -> Verdict {
    let mut improved = Vec::new();
    let mut neutral = Vec::new();
    let mut worse = Vec::new();
    let mut uncertain = Vec::new();

    for comparison in comparisons.iter().filter(|c| c.arm == "full_enabled") {
        let label = if comparison.baseline_below_floor {
            format!(
                "{} / {} ({:+.3} in metric units; the baseline {:.3} is below the noise floor, so \
                 a percentage would be misleading)",
                comparison.scenario,
                comparison.metric,
                comparison.absolute_delta,
                comparison.baseline_value
            )
        } else {
            format!(
                "{} / {} ({:+.1}%, range {:+.1}%..{:+.1}%)",
                comparison.scenario,
                comparison.metric,
                comparison.relative_delta * 100.0,
                comparison.sensitivity_range.0 * 100.0,
                comparison.sensitivity_range.1 * 100.0
            )
        };
        match comparison.direction {
            Direction::Unsupported => uncertain.push(format!("{label} — unsupported result")),
            _ if !comparison.direction_stable => {
                uncertain.push(format!("{label} — assumption-sensitive"))
            }
            Direction::Improved => improved.push(label),
            Direction::Worse => worse.push(label),
            Direction::NoMeaningfulDifference => neutral.push(label),
        }
    }

    let mut blocking = Vec::new();
    if !determinism.divergent.is_empty() {
        blocking.push(format!(
            "results were not deterministic across repeats: {}",
            determinism.divergent.join(", ")
        ));
    }
    if !controls.no_change_control_held {
        blocking.push("the no-change control changed machine state".to_string());
    }
    if !controls.harmful_control_detected {
        blocking.push(
            "the deliberately harmful control was not detected as harmful, so the result system \
             cannot be trusted to detect harm"
                .to_string(),
        );
    }
    if !safety.containment_clean {
        blocking.push("a write attempt escaped the simulation root".to_string());
    }
    if !safety.unrestored_without_a_fault.is_empty() {
        blocking.push(format!(
            "{} action(s) became active and did not restore in a scenario with no injected \
             fault: {}",
            safety.unrestored_without_a_fault.len(),
            safety.unrestored_without_a_fault.join("; ")
        ));
    }

    let mut rationale = Vec::new();
    rationale.push(format!(
        "{} improved, {} neutral, {} worse, {} uncertain measurements for the fully enabled arm.",
        improved.len(),
        neutral.len(),
        worse.len(),
        uncertain.len()
    ));
    rationale.push(format!(
        "{} of {} optid domains produced an action that actually became active on the simulated \
         machine; the rest are reported unsupported.",
        support.supported_domains.len(),
        KERNEL_DOMAINS.len() + 1
    ));

    let overall = if !blocking.is_empty() {
        "still inconclusive".to_string()
    } else if !worse.is_empty() && improved.len() > worse.len() {
        "theoretically beneficial with named regressions".to_string()
    } else if !worse.is_empty() && worse.len() >= improved.len() {
        "theoretically harmful in the modelled cases that regress".to_string()
    } else if improved.is_empty() {
        "theoretically neutral".to_string()
    } else {
        "theoretically beneficial".to_string()
    };

    Verdict {
        overall,
        rationale,
        blocking_failures: blocking,
        improved,
        neutral,
        worse,
        uncertain,
    }
}

/// Compile-time proof that the arm vocabulary the analysis assumes is the one
/// `scenarios` produces.
const _: fn() -> Vec<Arm> = super::scenarios::arms;
const _: ArmKind = ArmKind::DaemonAbsent;
