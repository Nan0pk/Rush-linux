//! The human-readable report. It answers the eight questions the evidence was
//! commissioned to answer, and nothing else.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::analysis::{Comparison, Direction};
use super::Bundle;

fn section(out: &mut String, title: &str) {
    let _ = writeln!(out, "\n## {title}\n");
}

fn pct(value: f64) -> String {
    format!("{:+.1}%", value * 100.0)
}

/// Render one comparison. When the baseline is smaller than the metric's noise
/// floor the relative figure is scaled by the floor, so the absolute change is
/// the honest number and is shown instead.
fn line(item: &Comparison) -> String {
    if item.baseline_below_floor {
        format!(
            "`{}` {:+.3} in metric units (baseline {:.3} is below the noise floor, so a percentage \
             would be misleading; enabled {:.3})",
            item.metric, item.absolute_delta, item.baseline_value, item.arm_value
        )
    } else {
        format!(
            "`{}` {} (baseline {:.3}, enabled {:.3}; assumption range {}..{})",
            item.metric,
            pct(item.relative_delta),
            item.baseline_value,
            item.arm_value,
            pct(item.sensitivity_range.0),
            pct(item.sensitivity_range.1)
        )
    }
}

fn group_by_scenario<'a>(
    comparisons: &'a [Comparison],
    arm: &str,
    wanted: Direction,
) -> BTreeMap<String, Vec<&'a Comparison>> {
    let mut out: BTreeMap<String, Vec<&Comparison>> = BTreeMap::new();
    for comparison in comparisons {
        if comparison.arm != arm || comparison.direction != wanted {
            continue;
        }
        if !comparison.direction_stable {
            continue;
        }
        out.entry(comparison.scenario.clone())
            .or_default()
            .push(comparison);
    }
    out
}

pub(crate) fn render(bundle: &Bundle) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# optid simulated evidence — fully enabled versus off\n"
    );
    let _ = writeln!(
        out,
        "> **Everything in this report is simulated and modelled.** No physical power, battery \
         life, temperature, hardware compatibility or real-world performance was measured, and \
         none is claimed. Every latency, throughput, energy and temperature figure is computed by \
         a documented model from the state of a simulated machine.\n"
    );
    let _ = writeln!(out, "**Question.** {}\n", bundle.question);
    let _ = writeln!(out, "**Answer.** {}\n", bundle.verdict.overall);
    for line in &bundle.verdict.rationale {
        let _ = writeln!(out, "- {line}");
    }

    section(&mut out, "Evidence class");
    let _ = writeln!(
        out,
        "`docs/research/0024-non-bare-metal-optid-validation-method.md` requires every result to \
         be named by its evidence class before it is used. This run produces exactly two:\n"
    );
    let _ = writeln!(
        out,
        "- **Deterministic software proof** — the policy decisions, per-domain gating, allowlist \
         and contract gates, write intent, read-back, restoration, crash handling, recovery and \
         failure behaviour. These are facts about optid's code.\n\
         - **Model-conditional estimate** — every latency, throughput, completed-work, pressure, \
         energy and temperature number, and therefore every improvement or regression in this \
         report. These hold *inside the declared model and its parameter range*, and nowhere else.\n"
    );
    let _ = writeln!(
        out,
        "It produces **no measured guest outcome** and **no physical measurement**. Nothing here \
         supports a claim about laptop watts, battery life, fan behaviour, suspend and resume, \
         firmware compatibility, or support for any named machine. Those claims remain blocked \
         until matching physical evidence exists.\n"
    );

    section(&mut out, "How this was produced");
    let _ = writeln!(
        out,
        "The unmodified production control loop (`crate::run`) was driven against a simulated \
         machine. Real optid code performed the sensing, workload classification, mode selection, \
         per-domain gating, hardware-allowlist checks, contract checks, capability checks, \
         transactional actuation, journalling, circuit-breaker accounting, shutdown restoration \
         and startup recovery. Only the machine underneath is modelled.\n"
    );
    let _ = writeln!(out, "- Simulation root: `{}`", bundle.simulation_root);
    let _ = writeln!(
        out,
        "- Simulated machine: {} ({} CPUs, {} devices, backlight {})",
        bundle.machine.name,
        bundle.machine.cpus,
        bundle.machine.devices.len(),
        bundle.machine.backlight
    );
    let _ = writeln!(out, "- Arms: {}", bundle.arms.len());
    let _ = writeln!(out, "- Scenarios: {}", bundle.scenarios.len());
    let _ = writeln!(out, "- Repeats per arm/scenario: {}", bundle.repeats);
    let _ = writeln!(
        out,
        "- Write attempts that left the simulation root: **{}**",
        bundle.total_host_write_attempts
    );
    let _ = writeln!(out, "\nContainment guards in force:\n");
    for guard in &bundle.containment_guards {
        let _ = writeln!(out, "- {guard}");
    }
    if !bundle.containment_violations.is_empty() {
        let _ = writeln!(out, "\n**Containment violations recorded:**\n");
        for violation in &bundle.containment_violations {
            let _ = writeln!(out, "- {violation}");
        }
    }

    section(
        &mut out,
        "1. Where fully enabled optid improved the modelled result",
    );
    let improved = group_by_scenario(&bundle.comparisons, "full_enabled", Direction::Improved);
    if improved.is_empty() {
        let _ = writeln!(
            out,
            "No workload showed a stable modelled improvement over optid off."
        );
    } else {
        for (scenario, items) in &improved {
            let _ = writeln!(out, "**{scenario}**\n");
            for item in items {
                let _ = writeln!(out, "- {}", line(item));
            }
            let _ = writeln!(out);
        }
    }

    section(&mut out, "2. Where it made no meaningful difference");
    let neutral = group_by_scenario(
        &bundle.comparisons,
        "full_enabled",
        Direction::NoMeaningfulDifference,
    );
    if neutral.is_empty() {
        let _ = writeln!(
            out,
            "Every judged measurement moved by more than the {:.0}% meaningfulness threshold.",
            super::analysis::MEANINGFUL_RELATIVE_DELTA * 100.0
        );
    } else {
        for (scenario, items) in &neutral {
            let names: Vec<&str> = items.iter().map(|item| item.metric.as_str()).collect();
            let _ = writeln!(out, "- **{scenario}**: {}", names.join(", "));
        }
        let _ = writeln!(
            out,
            "\nA change smaller than {:.0}% of the baseline is reported as no meaningful \
             difference.",
            super::analysis::MEANINGFUL_RELATIVE_DELTA * 100.0
        );
    }

    section(&mut out, "3. Where it made the modelled result worse");
    let worse = group_by_scenario(&bundle.comparisons, "full_enabled", Direction::Worse);
    if worse.is_empty() {
        let _ = writeln!(out, "No workload showed a stable modelled regression.");
    } else {
        for (scenario, items) in &worse {
            let _ = writeln!(out, "**{scenario}**\n");
            for item in items {
                let _ = writeln!(out, "- {}", line(item));
            }
            let _ = writeln!(out);
        }
    }

    section(&mut out, "4. Which optid actions caused each change");
    if bundle.attribution.is_empty() {
        let _ = writeln!(
            out,
            "No single-domain arm moved any metric past the meaningfulness threshold."
        );
    } else {
        let mut by_scenario: BTreeMap<&str, Vec<&super::analysis::Attribution>> = BTreeMap::new();
        for item in &bundle.attribution {
            by_scenario.entry(&item.scenario).or_default().push(item);
        }
        for (scenario, items) in by_scenario {
            let _ = writeln!(out, "**{scenario}**\n");
            let mut by_domain: BTreeMap<&str, Vec<&super::analysis::Attribution>> = BTreeMap::new();
            for item in items {
                by_domain.entry(&item.domain).or_default().push(item);
            }
            for (domain, entries) in by_domain {
                let effects: Vec<String> = entries
                    .iter()
                    .map(|entry| {
                        format!(
                            "{} {}{}",
                            entry.metric,
                            pct(entry.relative_delta),
                            if entry.direction_stable {
                                ""
                            } else {
                                " (assumption-sensitive)"
                            }
                        )
                    })
                    .collect();
                let _ = writeln!(out, "- `{domain}` → {}", effects.join("; "));
                if let Some(first) = entries.first() {
                    for action in first.actions.iter().take(4) {
                        let _ = writeln!(out, "  - {action}");
                    }
                }
            }
            let _ = writeln!(out);
        }
    }

    section(
        &mut out,
        "5. Did a combined configuration hide a harmful action?",
    );
    if bundle.masking.is_empty() {
        let _ = writeln!(
            out,
            "No. Every domain that was harmful in isolation was still visible as harmful, or \
             absent, in the fully enabled configuration."
        );
    } else {
        let _ = writeln!(
            out,
            "Yes. The following individually harmful actions do not show as harmful once every \
             domain runs together:\n"
        );
        for finding in &bundle.masking {
            let _ = writeln!(out, "- {}", finding.detail);
        }
    }

    section(
        &mut out,
        "6. Did every successful action restore correctly?",
    );
    let safety = &bundle.safety;
    let _ = writeln!(
        out,
        "- {} receipts recorded; {} actions became active on the simulated machine; {} of those \
         restored to their previous value.",
        safety.total_receipts, safety.active_receipts, safety.restored_receipts
    );
    if safety.unrestored_without_a_fault.is_empty() {
        let _ = writeln!(
            out,
            "- **In every scenario with no injected fault, every action that became active \
             restored to its previous value.**"
        );
    } else {
        let _ = writeln!(
            out,
            "- **Actions that did not restore even though nothing was injected:**\n"
        );
        for entry in &safety.unrestored_without_a_fault {
            let _ = writeln!(out, "  - {entry}");
        }
    }
    if !safety.unrestored_under_an_injected_fault.is_empty() {
        let _ = writeln!(
            out,
            "- Actions left applied by an injected fault (the fault is the cause, and each one is \
             the behaviour that fault is meant to produce):\n"
        );
        for entry in &safety.unrestored_under_an_injected_fault {
            let _ = writeln!(out, "  - {entry}");
        }
    }
    let _ = writeln!(
        out,
        "- Crash and reboot recovery exercised: {}. Everything restored after recovery: {}.",
        safety.crash_recovery_ran, safety.crash_recovery_restored_everything
    );
    let _ = writeln!(
        out,
        "- Injected failed-restoration scenario detected as a restoration failure: {}.",
        safety.failed_restoration_detected
    );
    if !safety.circuit_opened_in.is_empty() {
        let _ = writeln!(
            out,
            "- Circuit breaker opened in: {}.",
            safety.circuit_opened_in.join(", ")
        );
    }
    for note in &safety.notes {
        let _ = writeln!(out, "- {note}");
    }

    section(
        &mut out,
        "7. What is unsupported or too assumption-sensitive to judge",
    );
    let _ = writeln!(
        out,
        "**Domains that actually actuated, per configuration.** A domain that never produced an \
         action which read back as requested is unsupported here — it is not a passing test.\n"
    );
    for (arm, domains) in &bundle.support.active_domains_by_arm {
        let _ = writeln!(out, "- `{arm}`: {}", domains.join(", "));
    }
    if !bundle.support.escalated_domains_by_arm.is_empty() {
        let _ = writeln!(
            out,
            "\n**Domains that actuated in a configuration that had switched them off.** This can \
             only happen through the policy-reload fallback described in the findings below.\n"
        );
        for (arm, domains) in &bundle.support.escalated_domains_by_arm {
            let _ = writeln!(out, "- `{arm}`: {}", domains.join(", "));
        }
    }
    let _ = writeln!(
        out,
        "\n**Domains with no action that became active anywhere:**\n"
    );
    for (domain, reason) in &bundle.support.unsupported_domains {
        let _ = writeln!(out, "- `{domain}` — {reason}");
    }
    if !safety.inert_controls.is_empty() {
        let _ = writeln!(
            out,
            "\n**Inert controls** (the write was accepted and the machine ignored it; treated as \
             unsupported, never as a passing test): {}",
            safety.inert_controls.join(", ")
        );
    }
    if !safety.refused_writes.is_empty() {
        let _ = writeln!(
            out,
            "\n**Controls that refused every write:** {}",
            safety.refused_writes.join(", ")
        );
    }
    if !bundle.support.rejected_results.is_empty() {
        let _ = writeln!(out, "\n**Rejected results:**\n");
        for entry in bundle.support.rejected_results.iter().take(40) {
            let _ = writeln!(out, "- {entry}");
        }
    }
    if !bundle.verdict.uncertain.is_empty() {
        let _ = writeln!(out, "\n**Assumption-sensitive measurements:**\n");
        for entry in bundle.verdict.uncertain.iter().take(40) {
            let _ = writeln!(out, "- {entry}");
        }
    }

    section(&mut out, "Findings the comparison table does not show");
    if bundle.findings.is_empty() {
        let _ = writeln!(out, "None.");
    } else {
        for finding in &bundle.findings {
            let _ = writeln!(
                out,
                "### `{}` — {}\n\n{}\n",
                finding.id, finding.severity, finding.summary
            );
            for entry in finding.evidence.iter().take(10) {
                let _ = writeln!(out, "- {entry}");
            }
            let _ = writeln!(out);
        }
    }

    section(&mut out, "8. What the complete simulated evidence supports");
    let _ = writeln!(out, "**{}**\n", bundle.verdict.overall);
    if bundle.verdict.blocking_failures.is_empty() {
        let _ = writeln!(
            out,
            "No blocking failure was found in the evidence system itself: results were \
             deterministic across {} repeats, the no-change control did not move the machine, the \
             deliberately harmful control was detected as harmful, and no write escaped the \
             simulation root.",
            bundle.repeats
        );
    } else {
        let _ = writeln!(
            out,
            "The evidence system reported blocking failures, so the question cannot be answered \
             from this run:\n"
        );
        for failure in &bundle.verdict.blocking_failures {
            let _ = writeln!(out, "- {failure}");
        }
    }
    let _ = writeln!(
        out,
        "\nThis verdict is about a **modelled** system. It is evidence that the optid design is \
         or is not internally coherent and safe under the modelled assumptions. It is not evidence \
         about any physical machine, and it does not substitute for hardware validation."
    );

    section(&mut out, "Controls");
    let controls = &bundle.controls;
    let _ = writeln!(
        out,
        "- **No-change control** ({}): {}",
        controls.no_change_control_arms.join(", "),
        if controls.no_change_control_held {
            "held — no control value moved and no write left the simulated machine."
        } else {
            "FAILED — see violations below."
        }
    );
    for violation in &controls.no_change_violations {
        let _ = writeln!(out, "  - {violation}");
    }
    let _ = writeln!(
        out,
        "- **Deliberately harmful control** ({}): {}",
        controls.harmful_control_arm,
        if controls.harmful_control_detected {
            "detected as harmful."
        } else {
            "NOT detected — the result system cannot see harm."
        }
    );
    for entry in controls.harmful_control_evidence.iter().take(12) {
        let _ = writeln!(out, "  - {entry}");
    }

    section(&mut out, "Determinism");
    let _ = writeln!(
        out,
        "{} of {} arm/scenario groups produced byte-identical results across {} repeats.",
        bundle.determinism.identical_groups,
        bundle.determinism.compared_groups,
        bundle.determinism.repeats
    );
    for entry in &bundle.determinism.divergent {
        let _ = writeln!(out, "- diverged: {entry}");
    }

    section(&mut out, "Model assumptions and sensitivity");
    let _ = writeln!(
        out,
        "Every number in this report comes from the model in \
         `crates/optid/src/sim_evidence/model.rs`. The model reads only the simulated machine's \
         control values and the modelled environment. It has no knowledge of which arm is running.\n"
    );
    let _ = writeln!(
        out,
        "Results are reported under the `nominal` assumption set and re-evaluated under every set \
         below. A result whose direction reverses anywhere in this grid is reported as \
         assumption-sensitive rather than as a finding.\n"
    );
    for assumptions in &bundle.assumptions {
        let _ = writeln!(
            out,
            "- **{}** — {}",
            assumptions.id, assumptions.description
        );
    }
    let _ = writeln!(
        out,
        "\n**Stated limitation.** The sensitivity grid re-evaluates the recorded machine \
         trajectory. It does not re-run the closed loop, so it bounds model-parameter uncertainty, \
         not uncertainty in optid's own decision trajectory."
    );
    let _ = writeln!(
        out,
        "\n**Other stated assumptions.**\n\
         - One simulated machine shape is used throughout, so machine-to-machine variation is not \
           covered.\n\
         - Workload demand is exogenous: the offered load does not react to how fast the machine \
           serves it.\n\
         - Modelled temperature uses a single-node thermal model with one time constant.\n\
         - Battery drain is modelled from mean power and a fixed pack capacity; no charge \
           chemistry is modelled.\n\
         - The meaningfulness threshold is {:.0}% relative change.",
        super::analysis::MEANINGFUL_RELATIVE_DELTA * 100.0
    );

    section(&mut out, "Scenario catalogue");
    for scenario in &bundle.scenarios {
        let _ = writeln!(
            out,
            "- **{}**{} — {} ({} cycles of {}s, workload `{}`)",
            scenario.id,
            if scenario.safety_only {
                " *(safety)*"
            } else {
                ""
            },
            scenario.description,
            scenario.cycles,
            scenario.step_seconds,
            scenario.workload
        );
        for fault in &scenario.faults {
            let _ = writeln!(out, "  - fault: {fault}");
        }
    }

    section(&mut out, "Arm catalogue");
    for arm in &bundle.arms {
        let _ = writeln!(out, "- **{}** — {}", arm.id, arm.description);
    }

    let _ = writeln!(
        out,
        "\n---\n\nMachine-readable evidence: `evidence-bundle.json` (schema {}).",
        bundle.schema_version
    );
    out
}

pub(crate) fn render_console(bundle: &Bundle) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "optid simulated evidence — MODELLED RESULTS ONLY");
    let _ = writeln!(out, "verdict: {}", bundle.verdict.overall);
    for line in &bundle.verdict.rationale {
        let _ = writeln!(out, "  {line}");
    }
    let _ = writeln!(
        out,
        "determinism: {}/{} groups identical across {} repeats",
        bundle.determinism.identical_groups,
        bundle.determinism.compared_groups,
        bundle.determinism.repeats
    );
    let _ = writeln!(
        out,
        "containment: {} write attempts left the simulation root",
        bundle.total_host_write_attempts
    );
    let _ = writeln!(
        out,
        "controls: no-change held={} harmful detected={}",
        bundle.controls.no_change_control_held, bundle.controls.harmful_control_detected
    );
    for failure in &bundle.verdict.blocking_failures {
        let _ = writeln!(out, "BLOCKING: {failure}");
    }
    out
}
