use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HostInfo {
    pub kernel: String,
    pub cpu_model: String,
    pub dmi_board: String,
    pub battery_design_uwh: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RushInfo {
    pub optid_sha: String,
    pub contracts_sha256: String,
    pub rig_sha: String,
    pub rig_version: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ResolvedFloors {
    pub cpu_wakeup_latency_us: i64,
    pub device_resume_latency_us: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EnergyInfo {
    pub window_joules: f64,
    pub avg_watts: f64,
    pub counter: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RunRecord {
    pub schema_version: u32,
    pub host: HostInfo,
    pub rush: RushInfo,
    pub class_requested: String,
    pub class_observed: String,
    pub resolved_floors: ResolvedFloors,
    pub power_source: String,
    pub workload: String,
    pub metric: String,
    pub n: usize,
    pub samples: Option<Vec<u64>>,
    pub median: Option<f64>,
    pub p95: Option<f64>,
    pub iqr: Option<f64>,
    pub energy: Option<EnergyInfo>,
    pub started_at: String,
    pub warmup_runs: usize,
    pub anomalies: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OptctlStatus {
    pub workload_class: String,
    pub cpu_wakeup_latency: Option<i64>,
    pub device_resume_latency: Option<i64>,
    /// `boot.policy_load_state` from the daemon's status: `"ok"` when the
    /// configured `--config` policy loaded cleanly, `"defaulted"` /
    /// `"partial"` / `"invalid"` when the daemon silently fell back to its
    /// curated baseline. `None` when the field is absent (older schema).
    pub policy_load_state: Option<String>,
}
