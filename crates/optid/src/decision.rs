//! The `Decision` value: a renderable, explainable record of one optid loop
//! iteration. Rendered into the `status` and `decisions.log` state files that
//! `optctl status` / `optctl explain` read.

use std::path::Path;

use crate::action::Action;
use crate::actuators::runtime_pm::{with_selected_delay, RuntimePmDelaySelection};
use crate::envelope::{
    ActionOutcome, GateEvaluation, GateReasonCode, GateStage, PipelineStage, TargetOutcome,
};
use crate::policy::{Domain, DomainMode, EffectiveConfig};
use crate::sensors::{fmt_pressure, Snapshot};
use crate::workload::{Mode, WorkloadClass};

#[derive(Debug, Clone)]
pub(crate) struct Decision {
    pub(crate) mode: Mode,
    pub(crate) reasons: Vec<String>,
    pub(crate) actions: Vec<Action>,
    pub(crate) workload_class: WorkloadClass,
    pub(crate) workload_reason: String,
    pub(crate) cpu_wakeup_latency: Option<i64>,
    pub(crate) device_resume_latency: Option<i64>,
    /// F1 — The per-domain effective config used to filter this decision's
    /// actions. Rendered into the status report so `optctl status` shows
    /// exactly what optid is allowed to do per domain.
    pub(crate) effective_config: EffectiveConfig,
    /// F1 — Actions suppressed by the effective-mode gate when their
    /// domain was in `Observe` mode. Each entry is `(domain, description)`
    /// so the operator can see what optid *would* have done without
    /// those actions reaching the actuator. The `Decision::render` method
    /// emits a `suppressed_actions:` block that lists each would-be
    /// action's domain and human-readable description.
    ///
    /// `Off`-mode suppressions are deliberately not recorded here: the
    /// domain is invisible by design. `Observe` is the only mode that
    /// surfaces the would-be action.
    pub(crate) suppressed_actions: Vec<(Domain, Action)>,
    /// D1 — runtime-PM actions whose per-device delay evidence was refused
    /// by [`Decision::select_runtime_pm_delays`]. They are removed from
    /// `actions` and `suppressed_actions`, so nothing reports, plans, or
    /// writes the policy's proposed delay for them; `render` lists them
    /// under `refused_actions:` instead.
    pub(crate) refused_runtime_pm: Vec<RefusedRuntimePmDelay>,
}

/// D1 — a runtime-PM action refused because the device's verified
/// allowlist entry records an autosuspend delay outside the lever envelope.
#[derive(Debug, Clone)]
pub(crate) struct RefusedRuntimePmDelay {
    /// The action as the policy proposed it. Its delay is the proposal, not
    /// an intended value, and is never shown as one.
    pub(crate) action: Action,
    /// `true` when the action came from `suppressed_actions` (its domain is
    /// in observe mode), `false` when it was about to be applied.
    pub(crate) observe_only: bool,
    /// Why the delay was refused, in plain words, with no filesystem path.
    pub(crate) refusal: String,
}

impl Decision {
    /// D1 — choose the autosuspend delay of every runtime-PM action once per
    /// cycle, before the daemon renders the status report, builds the public
    /// outcomes, or plans circuit-breaker scopes. `select` is
    /// `Actuator::select_runtime_pm_delay` in production. A chosen delay
    /// replaces the policy's proposal in both `actions` and
    /// `suppressed_actions`, so the report, the journal, and the write all
    /// carry the same value; a refused one moves the action to
    /// `refused_runtime_pm`.
    pub(crate) fn select_runtime_pm_delays(
        &mut self,
        select: impl Fn(&Path, i32) -> RuntimePmDelaySelection,
    ) {
        for action in std::mem::take(&mut self.actions) {
            match with_selected_delay(&action, &select) {
                Ok(chosen) => self.actions.push(chosen),
                Err(refusal) => self.refused_runtime_pm.push(RefusedRuntimePmDelay {
                    action,
                    observe_only: false,
                    refusal,
                }),
            }
        }
        for (domain, action) in std::mem::take(&mut self.suppressed_actions) {
            match with_selected_delay(&action, &select) {
                Ok(chosen) => self.suppressed_actions.push((domain, chosen)),
                Err(refusal) => self.refused_runtime_pm.push(RefusedRuntimePmDelay {
                    action,
                    observe_only: true,
                    refusal,
                }),
            }
        }
    }

    /// D1 — the public outcome for each refused runtime-PM action, built the
    /// same way the daemon builds outcomes for the actions it kept (applied,
    /// not armed, or observe-only), then marked refused. The desired value
    /// names no delay, because none was intended.
    pub(crate) fn refused_runtime_pm_outcomes(
        &self,
        cycle_apply_armed: bool,
    ) -> Vec<ActionOutcome> {
        self.refused_runtime_pm
            .iter()
            .map(|refused| {
                let action = &refused.action;
                let mut outcome = if refused.observe_only {
                    ActionOutcome::suppressed(action, DomainMode::Observe, false)
                } else if cycle_apply_armed {
                    let mut outcome = ActionOutcome::new(action);
                    outcome.gates.push(GateEvaluation::allowed(
                        GateStage::DomainMode,
                        GateReasonCode::DomainActuate,
                    ));
                    outcome.gates.push(GateEvaluation::allowed(
                        GateStage::ApplyArmed,
                        GateReasonCode::ApplyArmed,
                    ));
                    outcome.targets.push(TargetOutcome::denied(
                        action.stable_target_id(),
                        PipelineStage::Write,
                        refused.refusal.clone(),
                    ));
                    outcome
                } else {
                    ActionOutcome::suppressed(action, DomainMode::Actuate, false)
                };
                outcome.desired.value = "control=auto;autosuspend_delay_ms=refused".to_string();
                for target in &mut outcome.targets {
                    target.detail = Some(match target.detail.take() {
                        Some(existing) if existing != refused.refusal => {
                            format!("{existing}; {}", refused.refusal)
                        }
                        _ => refused.refusal.clone(),
                    });
                }
                outcome
            })
            .collect()
    }

    pub(crate) fn render(&self, snapshot: &Snapshot) -> String {
        let mut out = String::new();
        out.push_str(&format!("timestamp={}\n", snapshot.timestamp));
        out.push_str(&format!("mode={}\n", self.mode));
        out.push_str(&format!("on_ac={:?}\n", snapshot.on_ac));
        out.push_str(&format!("battery_pct={:?}\n", snapshot.battery_pct));
        out.push_str(&format!("thermal_c={:?}\n", snapshot.thermal_c()));
        // T1 — operator-visible thermal evidence (state, ratio, selected
        // sensors/temps, fan RPM, concise reasons). Does not dump raw
        // readings. Production consumer of collect → budget → render.
        out.push_str(&crate::thermal::render_thermal_status(
            &snapshot.thermal_budget,
        ));
        // Sensor counts (not raw dumps) so operators can see discovery volume.
        out.push_str(&format!(
            "thermal_sensor_count={}\n",
            snapshot.thermal_sensors.len()
        ));
        out.push_str(&format!(
            "fan_sensor_count={}\n",
            snapshot.fan_sensors.len()
        ));
        out.push_str(&format!("loadavg_1={:?}\n", snapshot.loadavg_1));
        out.push_str(&format!(
            "cpu_pressure={}\n",
            fmt_pressure(snapshot.cpu_pressure)
        ));
        out.push_str(&format!(
            "memory_pressure={}\n",
            fmt_pressure(snapshot.memory_pressure)
        ));
        out.push_str(&format!(
            "io_pressure={}\n",
            fmt_pressure(snapshot.io_pressure)
        ));
        out.push_str(&format!("workload_class={}\n", self.workload_class));
        out.push_str(&format!("workload_reason={}\n", self.workload_reason));

        match self.cpu_wakeup_latency {
            Some(v) => out.push_str(&format!("cpu_wakeup_latency={}\n", v)),
            None => out.push_str("cpu_wakeup_latency=None\n"),
        }
        match self.device_resume_latency {
            Some(v) => out.push_str(&format!("device_resume_latency={}\n", v)),
            None => out.push_str("device_resume_latency=None\n"),
        }

        out.push_str("reasons:\n");
        for reason in &self.reasons {
            out.push_str(&format!("- {reason}\n"));
        }
        out.push_str("actions:\n");
        for action in &self.actions {
            out.push_str(&format!("- {}\n", action.describe()));
        }
        // F1 — surface observe-mode would-be actions so the operator
        // can see exactly what optid would have done. This is the
        // repair for the F1 merged_incomplete blocking reason
        // "Observe mode loses the would-be action". Off-mode
        // suppressions are intentionally absent: the domain is
        // invisible by design.
        if !self.suppressed_actions.is_empty() {
            out.push_str("suppressed_actions:\n");
            // `&self.suppressed_actions` iterates as `&(Domain, String)`,
            // so the pattern must be `&(domain, description)`.
            for (domain, action) in &self.suppressed_actions {
                out.push_str(&format!(
                    "- domain={} would_act={}\n",
                    domain.as_str(),
                    action.describe()
                ));
            }
        }
        // D1 — runtime-PM actions refused because the device's verified
        // delay is outside the lever envelope. The policy's proposed delay
        // is deliberately not printed: it was never going to be written.
        if !self.refused_runtime_pm.is_empty() {
            out.push_str("refused_actions:\n");
            for refused in &self.refused_runtime_pm {
                let target = match &refused.action {
                    Action::RuntimePm { device_dir, .. } => device_dir.display().to_string(),
                    other => other.stable_target_id(),
                };
                out.push_str(&format!(
                    "- domain=runtime_pm target={target}{} refused: {}\n",
                    if refused.observe_only {
                        " (observe mode)"
                    } else {
                        ""
                    },
                    refused.refusal
                ));
            }
        }
        // F1 — append the effective per-domain config so `optctl status`
        // surfaces the runtime mode of every domain. This is the
        // "EffectiveConfig object consumed by policy and exposed to optctl"
        // contract from the F1 plan.
        out.push_str("effective_config:\n");
        out.push_str(&self.effective_config.render());
        out
    }
}
