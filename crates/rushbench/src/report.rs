use std::fs;
use std::path::Path;

use crate::contracts::parse_contracts_toml;
use crate::types::RunRecord;
use crate::utils::{find_repo_file, get_utc_timestamp};

pub fn run_report(results_dir: &str) -> Result<String, String> {
    use std::fmt::Write as _;
    let mut out = String::new();
    let results_path = Path::new(results_dir);
    if !results_path.exists() {
        return Err(format!("Results directory {} does not exist", results_dir));
    }

    let contracts_path = find_repo_file("config/optid/contracts.toml");
    let contracts = contracts_path
        .map(|p| parse_contracts_toml(&p))
        .unwrap_or_default();

    let mut records = Vec::new();
    find_json_files(results_path, &mut records)?;

    if records.is_empty() {
        writeln!(&mut out, "No measurement records found in {}", results_dir).unwrap();
        return Ok(out);
    }

    writeln!(&mut out, "# Rush Linux Contract Validation Report").unwrap();
    writeln!(&mut out, "Generated at: {}\n", get_utc_timestamp()).unwrap();

    writeln!(&mut out, "## Summary Table").unwrap();
    writeln!(&mut out, "| Class | Workload | Metric | N | Median | P95 | IQR | Avg Power (W) | Status / Violations |").unwrap();
    writeln!(&mut out, "|---|---|---|---|---|---|---|---|---|").unwrap();

    records.sort_by(|a, b| {
        a.class_requested
            .cmp(&b.class_requested)
            .then(a.workload.cmp(&b.workload))
    });

    let mut idle_power: Option<f64> = None;
    let mut interactive_power: Option<f64> = None;

    for r in &records {
        let mut status_flags = Vec::new();

        if r.anomalies.contains(&"insufficient_n".to_string()) || r.n < 5 {
            status_flags.push("insufficient_n");
        }
        if r.anomalies.contains(&"class_mismatch".to_string()) {
            status_flags.push("class_mismatch");
        }
        if r.anomalies.contains(&"unsupported_here".to_string()) {
            status_flags.push("unsupported_here");
        }
        if r.anomalies.contains(&"probe_failed".to_string()) {
            status_flags.push("probe_failed");
        }

        if r.class_observed == "latency-critical" && r.workload == "cyclictest" {
            if let Some(med_val) = r.median {
                if let Some(contract) = contracts.get("latency-critical") {
                    if med_val > contract.cpu_wakeup_latency as f64 {
                        status_flags.push("budget_violation");
                    }
                }
            }
        }

        if r.power_source == "battery" && !status_flags.contains(&"insufficient_n") {
            if r.class_observed == "idle" {
                if let Some(ref e) = r.energy {
                    idle_power = Some(e.avg_watts);
                }
            } else if r.class_observed == "interactive" {
                if let Some(ref e) = r.energy {
                    interactive_power = Some(e.avg_watts);
                }
            }
        }

        let median_str = r
            .median
            .map(|v| v.to_string())
            .unwrap_or_else(|| "N/A".to_string());
        let p95_str = r
            .p95
            .map(|v| v.to_string())
            .unwrap_or_else(|| "N/A".to_string());
        let iqr_str = r
            .iqr
            .map(|v| v.to_string())
            .unwrap_or_else(|| "N/A".to_string());

        let power_str = r
            .energy
            .as_ref()
            .map(|e| format!("{:.2} W", e.avg_watts))
            .unwrap_or_else(|| "N/A".to_string());

        let status_str = if status_flags.is_empty() {
            "OK".to_string()
        } else {
            status_flags.join(", ")
        };

        writeln!(
            &mut out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.class_requested,
            r.workload,
            r.metric,
            r.n,
            median_str,
            p95_str,
            iqr_str,
            power_str,
            status_str
        )
        .unwrap();
    }
    writeln!(&mut out).unwrap();

    writeln!(&mut out, "## Energy Analysis").unwrap();
    if let (Some(idle_w), Some(int_w)) = (idle_power, interactive_power) {
        writeln!(&mut out, "- Idle average power draw: {:.2} W", idle_w).unwrap();
        writeln!(&mut out, "- Interactive average power draw: {:.2} W", int_w).unwrap();
        if idle_w >= int_w {
            writeln!(&mut out, "- **Warning:** idle power draw ({:.2} W) is NOT less than interactive power draw ({:.2} W)!", idle_w, int_w).unwrap();
        } else {
            writeln!(
                &mut out,
                "- Idle power draw is less than interactive power draw (expected behavior)."
            )
            .unwrap();
        }
    } else {
        writeln!(
            &mut out,
            "- Note: insufficient battery run data to compare idle vs interactive power draw."
        )
        .unwrap();
    }
    writeln!(&mut out).unwrap();

    writeln!(&mut out, "## Contract Verification").unwrap();
    let mut lc_checked = false;
    for r in &records {
        if r.class_observed == "latency-critical" && r.workload == "cyclictest" {
            lc_checked = true;
            if let Some(med_val) = r.median {
                if let Some(contract) = contracts.get("latency-critical") {
                    writeln!(
                        &mut out,
                        "- Latency-critical cyclictest median: {} us (Contract Floor: {} us)",
                        med_val, contract.cpu_wakeup_latency
                    )
                    .unwrap();
                    if med_val > contract.cpu_wakeup_latency as f64 {
                        writeln!(&mut out, "  - **BUDGET VIOLATION DETECTED**: Observed latency ({} us) exceeds contract limit ({} us)!", med_val, contract.cpu_wakeup_latency).unwrap();
                    } else {
                        writeln!(
                            &mut out,
                            "  - Pass: Observed latency fits within the contract budget."
                        )
                        .unwrap();
                    }
                }
            } else {
                writeln!(
                    &mut out,
                    "- Latency-critical cyclictest run has no valid median data."
                )
                .unwrap();
            }
        }
    }
    if !lc_checked {
        writeln!(
            &mut out,
            "- Note: no latency-critical cyclictest results found in the dataset."
        )
        .unwrap();
    }

    Ok(out)
}

fn find_json_files(dir: &Path, records: &mut Vec<RunRecord>) -> Result<(), String> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                find_json_files(&path, records)?;
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                if let Ok(rec) = serde_json::from_str::<RunRecord>(&content) {
                    records.push(rec);
                }
            }
        }
    }
    Ok(())
}
