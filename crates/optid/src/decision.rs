//! The `Decision` value: a renderable, explainable record of one optid loop
//! iteration. Rendered into the `status` and `decisions.log` state files that
//! `optctl status` / `optctl explain` read.

use crate::action::Action;
use crate::policy::{Domain, EffectiveConfig};
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
    pub(crate) suppressed_actions: Vec<(Domain, String)>,
}

impl Decision {
    pub(crate) fn render(&self, snapshot: &Snapshot) -> String {
        let mut out = String::new();
        out.push_str(&format!("timestamp={}\n", snapshot.timestamp));
        out.push_str(&format!("mode={}\n", self.mode));
        out.push_str(&format!("on_ac={:?}\n", snapshot.on_ac));
        out.push_str(&format!("battery_pct={:?}\n", snapshot.battery_pct));
        out.push_str(&format!("thermal_c={:?}\n", snapshot.thermal_c()));
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
            for &(domain, ref description) in &self.suppressed_actions {
                out.push_str(&format!(
                    "- domain={} would_act={}\n",
                    domain.as_str(),
                    description
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
