//! The `Decision` value: a renderable, explainable record of one optid loop
//! iteration. Rendered into the `status` and `decisions.log` state files that
//! `optctl status` / `optctl explain` read.

use crate::action::Action;
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
        out
    }
}
