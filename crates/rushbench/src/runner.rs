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

pub fn write_class_mismatch_record(
    requested: &str,
    observed: &str,
    workload: &str,
    metric: &str,
    n: usize,
    power_source: &str,
) -> Result<(), String> {
    let start_time_str = get_utc_timestamp();
    write_record(
        requested,
        observed,
        workload,
        metric,
        n,
        None,
        None,
        &start_time_str,
        0,
        vec!["class_mismatch".to_string()],
        power_source,
        -1,
        -1,
    )
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
        write_class_mismatch_record(
            class,
            &status.workload_class,
            &workload_name,
            &metric_name,
            n,
            power_source,
        )?;
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
        write_class_mismatch_record(
            class,
            &status.workload_class,
            &workload_name,
            &metric_name,
            n,
            power_source,
        )?;
        return Err(format!(
            "class_mismatch: resolved floors do not match contracts.toml. Expected (cpu={}, dev={}), Observed (cpu={}, dev={})",
            expected_contract.cpu_wakeup_latency, expected_contract.device_resume_latency,
            observed_cpu, observed_dev
        ));
    }

    if !check_apply_in_effect() {
        write_class_mismatch_record(
            class,
            &status.workload_class,
            &workload_name,
            &metric_name,
            n,
            power_source,
        )?;
        return Err(
            "class_mismatch: optid is not running in enforcement mode (--apply not in effect)"
                .to_string(),
        );
    }

    let warmup_runs = 2;
    for _ in 0..warmup_runs {
        let _ = run_probe_for_metric(&metric_name);
    }

    let mut samples = Vec::new();
    let mut anomalies = Vec::new();

    let start_time_str = get_utc_timestamp();

    let energy_start = energy_source
        .sample()
        .map_err(|e| format!("Failed to sample energy: {e}"))?;

    let mut probe_failed_msg = None;
    let mut unsupported_msg = None;

    // Phase 1: Collect exactly n samples
    let original_n = n; // Preserve requested sample count
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

    // Phase 2: Extend sampling for real energy source to ensure energy window advances
    let is_real_energy = std::env::var("RUSHBENCH_MOCK_ENERGY_JOULES").is_err();
    if unsupported_msg.is_none() && probe_failed_msg.is_none() && is_real_energy {
        let start_instant = std::time::Instant::now();
        let min_duration = 30.0; // seconds
        while start_instant.elapsed().as_secs_f64() < min_duration {

            match run_probe_for_metric(&metric_name) {
                ProbeResult::Success(val) => samples.push(val),
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
            original_n,
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
            original_n,
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

    let mut sorted = samples.clone();
    sorted.sort_unstable();

    if actual_n < 5 {
        anomalies.push("insufficient_n".to_string());
    }

    write_record(
        class,
        &status.workload_class,
        &workload_name,
        &metric_name,
        original_n,
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
