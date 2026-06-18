use std::env;
use std::fs;
use std::path::PathBuf;

use crate::contracts::{check_apply_in_effect, get_optctl_status_json, parse_contracts_toml};
use crate::energy::{calculate_window, read_on_ac, EnergySource};
use crate::probes::{run_probe_for_metric, ProbeResult};
use crate::types::{EnergyInfo, HostInfo, OptctlStatus, ResolvedFloors, RunRecord, RushInfo};
use crate::utils::{
    find_repo_file, get_battery_design_uwh, get_contracts_sha256, get_cpu_model, get_dmi_board,
    get_git_sha, get_host_folder_name, get_kernel_version, get_utc_timestamp, percentile,
};

pub fn resolve_workload_and_metric(w: &str) -> Result<(String, String), String> {
    match w {
        "foreground-launch" | "foreground-launch-ms" => Ok((
            "foreground-launch".to_string(),
            "foreground-launch-ms".to_string(),
        )),
        "cyclictest" | "cyclictest-max-us" => {
            Ok(("cyclictest".to_string(), "cyclictest-max-us".to_string()))
        }
        "input-latency-p95-ms" | "input-latency-p95" => Ok((
            "input-latency".to_string(),
            "input-latency-p95-ms".to_string(),
        )),
        "input-latency-p99-ms" | "input-latency-p99" | "input-latency" => Ok((
            "input-latency".to_string(),
            "input-latency-p99-ms".to_string(),
        )),
        "psi-cpu-avg10" | "psi-cpu" => Ok(("psi-cpu".to_string(), "psi-cpu-avg10".to_string())),
        "psi-io-avg10" | "psi-io" => Ok(("psi-io".to_string(), "psi-io-avg10".to_string())),
        _ => Err(format!("Unknown workload: {w}")),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn write_record(
    class_requested: &str,
    class_observed: &str,
    workload: &str,
    metric: &str,
    n: usize,
    samples: Option<Vec<u64>>,
    energy: Option<EnergyInfo>,
    started_at: &str,
    warmup_runs: usize,
    anomalies: Vec<String>,
    power_source: &str,
    cpu_floor: i64,
    dev_floor: i64,
) -> Result<(), String> {
    let host_folder = get_host_folder_name();
    let utc_date = started_at.split('T').next().unwrap_or("unknown");

    let default_root = find_repo_file("VERSION")
        .map(|p| p.parent().unwrap().join("benchmarks").join("results"))
        .unwrap_or_else(|| PathBuf::from("benchmarks/results"));

    // Base results directory can be overridden by RUSHBENCH_STATE_DIR for testing
    let results_root = if let Ok(dir) = env::var("RUSHBENCH_STATE_DIR") {
        PathBuf::from(dir)
    } else {
        default_root.clone()
    };

    let target_dir = results_root
        .join(utc_date)
        .join(&host_folder)
        .join(class_requested);

    fs::create_dir_all(&target_dir).map_err(|e| format!("Failed to create results dir: {e}"))?;

    let target_file = target_dir.join(format!("{workload}.json"));

    let (median, p95, iqr) = if let Some(ref s) = samples {
        let mut sorted = s.clone();
        sorted.sort_unstable();
        let med = percentile(&sorted, 0.5);
        let p95_val = percentile(&sorted, 0.95);
        let q1 = percentile(&sorted, 0.25);
        let q3 = percentile(&sorted, 0.75);
        (
            Some((med * 100.0).round() / 100.0),
            Some((p95_val * 100.0).round() / 100.0),
            Some(((q3 - q1) * 100.0).round() / 100.0),
        )
    } else {
        (None, None, None)
    };

    let git_sha = get_git_sha().unwrap_or_else(|_| "unknown".to_string());

    let record = RunRecord {
        schema_version: 1,
        host: HostInfo {
            kernel: get_kernel_version(),
            cpu_model: get_cpu_model(),
            dmi_board: get_dmi_board(),
            battery_design_uwh: get_battery_design_uwh(),
        },
        rush: RushInfo {
            optid_sha: git_sha.clone(),
            contracts_sha256: get_contracts_sha256().unwrap_or_else(|_| "unknown".to_string()),
            rig_sha: git_sha,
            rig_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        class_requested: class_requested.to_string(),
        class_observed: class_observed.to_string(),
        resolved_floors: ResolvedFloors {
            cpu_wakeup_latency_us: cpu_floor,
            device_resume_latency_us: dev_floor,
        },
        power_source: power_source.to_string(),
        workload: workload.to_string(),
        metric: metric.to_string(),
        n,
        samples,
        median,
        p95,
        iqr,
        energy,
        started_at: started_at.to_string(),
        warmup_runs,
        anomalies,
    };

    let json_str = serde_json::to_string_pretty(&record)
        .map_err(|e| format!("Failed to serialize RunRecord: {e}"))?;

    fs::write(&target_file, &json_str).map_err(|e| format!("Failed to write result file: {e}"))?;

    println!("Wrote results to {}", target_file.display());
    Ok(())
}

pub fn run_cell(class: &str, workload: &str, n: usize, ac_ok: bool) -> Result<(), String> {
    let (workload_name, metric_name) = resolve_workload_and_metric(workload)?;

    let energy_source = EnergySource::detect().map_err(|e| format!("no_energy_counter: {}", e))?;

    let on_ac = read_on_ac();
    let power_source = if on_ac == Some(true) { "ac" } else { "battery" };
    if power_source == "ac" && !ac_ok {
        return Err("Refusing to run on AC power. Use --ac-ok to override.".to_string());
    }

    if env::var("RUSHBENCH_OPTCTL_STATUS_JSON").is_err() {
        let pin_res = std::process::Command::new("optctl")
            .arg("pin")
            .arg("rushbench")
            .arg(class)
            .output();
        match pin_res {
            Ok(out) => {
                if !out.status.success() {
                    return Err(format!(
                        "optctl pin failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
            }
            Err(e) => return Err(format!("Failed to execute optctl pin: {e}")),
        }
    }

    let status_json_str = get_optctl_status_json()?;
    let status: OptctlStatus = serde_json::from_str(&status_json_str)
        .map_err(|e| format!("Failed to parse optctl status JSON: {e}"))?;

    if status.workload_class != class {
        return Err(format!(
            "class_mismatch: requested={}, observed={}",
            class, status.workload_class
        ));
    }

    let contracts_path = find_repo_file("config/optid/contracts.toml")
        .ok_or_else(|| "Could not find contracts.toml".to_string())?;
    let contracts = parse_contracts_toml(&contracts_path);
    let expected_contract = contracts
        .get(class)
        .ok_or_else(|| format!("Class {class} not found in contracts.toml"))?;

    let observed_cpu = status.cpu_wakeup_latency.unwrap_or(-1);
    let observed_dev = status.device_resume_latency.unwrap_or(-1);
    if observed_cpu != expected_contract.cpu_wakeup_latency
        || observed_dev != expected_contract.device_resume_latency
    {
        return Err(format!(
            "class_mismatch: resolved floors do not match contracts.toml. Expected (cpu={}, dev={}), Observed (cpu={}, dev={})",
            expected_contract.cpu_wakeup_latency, expected_contract.device_resume_latency,
            observed_cpu, observed_dev
        ));
    }

    if !check_apply_in_effect() {
        return Err(
            "class_mismatch: optid is not running in enforcement mode (--apply not in effect)"
                .to_string(),
        );
    }

    let is_real_energy = env::var("RUSHBENCH_MOCK_ENERGY_JOULES").is_err();
    let is_psi_workload = metric_name.starts_with("psi-");

    // Time-based warmup before the energy window.
    // PSI probes need ≥15s for real pressure to accumulate in the kernel counters.
    // RUSHBENCH_WARMUP_SEC overrides the default (set to 0 in fast tests).
    // Mock mode (RUSHBENCH_MOCK_ENERGY_JOULES set) uses 2 count-based iterations.
    let warmup_runs = if is_real_energy {
        let default_sec = if is_psi_workload { 15.0 } else { 3.0 };
        let warmup_sec = env::var("RUSHBENCH_WARMUP_SEC")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(default_sec);
        let warmup_start = std::time::Instant::now();
        let mut count = 0usize;
        while warmup_start.elapsed().as_secs_f64() < warmup_sec {
            let _ = run_probe_for_metric(&metric_name);
            count += 1;
        }
        count
    } else {
        for _ in 0..2 {
            let _ = run_probe_for_metric(&metric_name);
        }
        2
    };

    let mut samples = Vec::new();
    let mut anomalies = Vec::new();
    let start_time_str = get_utc_timestamp();

    let energy_start = energy_source
        .sample()
        .map_err(|e| format!("Failed to sample energy: {e}"))?;

    let mut probe_failed_msg = None;
    let mut unsupported_msg = None;

    let min_duration = env::var("RUSHBENCH_MIN_ENERGY_WINDOW_SEC")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(30.0);

    // Phase 1: Filler loop — keeps the workload active for the bulk of the energy window
    // so the system is in a representative steady state when samples are taken.
    if is_real_energy {
        let filler_start = std::time::Instant::now();
        while filler_start.elapsed().as_secs_f64() < min_duration {
            match run_probe_for_metric(&metric_name) {
                ProbeResult::Success(_) => {}
                ProbeResult::UnsupportedHere(msg) => {
                    unsupported_msg = Some(msg);
                    break;
                }
                ProbeResult::Failed(msg) => {
                    probe_failed_msg = Some(msg);
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    // Phase 2: Collect exactly n samples at the end of the energy window.
    if unsupported_msg.is_none() && probe_failed_msg.is_none() {
        for _ in 0..n {
            match run_probe_for_metric(&metric_name) {
                ProbeResult::Success(val) => {
                    samples.push(val);
                }
                ProbeResult::UnsupportedHere(msg) => {
                    unsupported_msg = Some(msg);
                    break;
                }
                ProbeResult::Failed(msg) => {
                    probe_failed_msg = Some(msg);
                    break;
                }
            }
        }
    }

    let energy_end = energy_source
        .sample()
        .map_err(|e| format!("Failed to sample energy: {e}"))?;

    let energy_info = if unsupported_msg.is_none() && probe_failed_msg.is_none() {
        match calculate_window(&energy_source, &energy_start, &energy_end) {
            Ok(info) => Some(info),
            Err(anomaly) => {
                anomalies.push(anomaly);
                None
            }
        }
    } else {
        None
    };

    let actual_n = samples.len();

    if let Some(msg) = unsupported_msg {
        anomalies.push("unsupported_here".to_string());
        write_record(
            class,
            &status.workload_class,
            &workload_name,
            &metric_name,
            n,
            None,
            energy_info,
            &start_time_str,
            warmup_runs,
            anomalies,
            power_source,
            observed_cpu,
            observed_dev,
        )?;
        return Err(format!("Unsupported here: {msg}"));
    }

    if let Some(msg) = probe_failed_msg {
        anomalies.push("probe_failed".to_string());
        write_record(
            class,
            &status.workload_class,
            &workload_name,
            &metric_name,
            n,
            None,
            energy_info,
            &start_time_str,
            warmup_runs,
            anomalies,
            power_source,
            observed_cpu,
            observed_dev,
        )?;
        return Err(format!("Probe failed: {msg}"));
    }

    if actual_n < 5 {
        anomalies.push("insufficient_n".to_string());
    }

    write_record(
        class,
        &status.workload_class,
        &workload_name,
        &metric_name,
        n,
        Some(samples),
        energy_info,
        &start_time_str,
        warmup_runs,
        anomalies,
        power_source,
        observed_cpu,
        observed_dev,
    )?;

    Ok(())
}
