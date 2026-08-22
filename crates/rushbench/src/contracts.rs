use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

use crate::types::OptctlStatus;

#[derive(Debug, Clone)]
pub struct ContractItem {
    pub cpu_wakeup_latency: i64,
    pub device_resume_latency: i64,
}

pub fn get_optctl_status_json() -> Result<String, String> {
    if let Ok(mocked) = env::var("RUSHBENCH_OPTCTL_STATUS_JSON") {
        return Ok(mocked);
    }
    let output = std::process::Command::new("optctl")
        .arg("status")
        .arg("--json")
        .output()
        .map_err(|e| format!("failed to execute optctl: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "optctl status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn is_optid_applying() -> bool {
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
            {
                if let Ok(comm) = fs::read_to_string(path.join("comm")) {
                    if comm.trim() == "optid" {
                        if let Ok(cmdline) = fs::read_to_string(path.join("cmdline")) {
                            let args: Vec<&str> = cmdline.split('\0').collect();
                            if args.contains(&"--apply") {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Read a workload class and its resolved latency floors out of
/// `optctl status --json`, accepting both schema generations.
///
/// `optctl` moved to `schema_version = 2`, which nests the fields under
/// `decision` (`decision.workload_class`, `decision.contract.*_us`) where
/// version 1 had them at the top level. `OptctlStatus` still described the flat
/// v1 shape, so a strict deserialize failed against every current daemon: the
/// preset silently recorded `optid_absent` for a live daemon, and
/// `rushbench run --class` aborted with a parse error.
pub fn parse_optctl_status(json: &str) -> Result<OptctlStatus, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("optctl status is not JSON: {e}"))?;
    let schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let decision = value.get("decision");
    let contract = decision.and_then(|d| d.get("contract"));

    let pick = |nested: Option<&serde_json::Value>, nested_key: &str, flat_key: &str| {
        nested
            .and_then(|n| n.get(nested_key))
            .or_else(|| value.get(flat_key))
            .cloned()
    };

    let workload_class = pick(decision, "workload_class", "workload_class")
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            format!("optctl status (schema_version {schema}) carries no workload_class")
        })?;

    Ok(OptctlStatus {
        workload_class,
        cpu_wakeup_latency: pick(contract, "cpu_wakeup_latency_us", "cpu_wakeup_latency")
            .as_ref()
            .and_then(serde_json::Value::as_i64),
        device_resume_latency: pick(
            contract,
            "device_resume_latency_us",
            "device_resume_latency",
        )
        .as_ref()
        .and_then(serde_json::Value::as_i64),
    })
}

pub fn check_apply_in_effect() -> bool {
    if let Ok(override_val) = env::var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE") {
        return override_val == "true";
    }
    is_optid_applying()
}

pub fn parse_contracts_toml(path: &Path) -> HashMap<String, ContractItem> {
    let mut map = HashMap::new();
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut current_class = String::new();
    let mut cpu_val = 0;
    let mut dev_val = 0;

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
            if !current_class.is_empty() {
                map.insert(
                    current_class.clone(),
                    ContractItem {
                        cpu_wakeup_latency: cpu_val,
                        device_resume_latency: dev_val,
                    },
                );
            }
            let section = &line[1..line.len() - 1];
            if let Some(class) = section.strip_prefix("contracts.") {
                current_class = class.trim().to_string();
            } else {
                current_class = String::new();
            }
            cpu_val = 0;
            dev_val = 0;
        } else if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val_str = v.trim();
            if let Ok(val) = val_str.parse::<i64>() {
                if key == "cpu_wakeup_latency" {
                    cpu_val = val;
                } else if key == "device_resume_latency" {
                    dev_val = val;
                }
            }
        }
    }
    if !current_class.is_empty() {
        map.insert(
            current_class,
            ContractItem {
                cpu_wakeup_latency: cpu_val,
                device_resume_latency: dev_val,
            },
        );
    }
    map
}

#[cfg(test)]
mod status_tests {
    use super::*;

    /// The shape `optctl status --json` emits today.
    const V2: &str = r#"{
        "schema_version": 2,
        "pipeline_stage": "complete",
        "boot": { "apply_armed": false },
        "decision": {
            "workload_class": "idle",
            "contract": {
                "workload_class": "idle",
                "cpu_wakeup_latency_us": 100000,
                "device_resume_latency_us": 1000000
            }
        }
    }"#;

    /// The flat shape `OptctlStatus` was written against.
    const V1: &str = r#"{
        "workload_class": "interactive",
        "cpu_wakeup_latency": 250,
        "device_resume_latency": 5000
    }"#;

    #[test]
    fn reads_the_nested_v2_schema() {
        let status = parse_optctl_status(V2).expect("v2 status parses");
        assert_eq!(status.workload_class, "idle");
        assert_eq!(status.cpu_wakeup_latency, Some(100_000));
        assert_eq!(status.device_resume_latency, Some(1_000_000));
    }

    #[test]
    fn still_reads_the_flat_v1_schema() {
        let status = parse_optctl_status(V1).expect("v1 status parses");
        assert_eq!(status.workload_class, "interactive");
        assert_eq!(status.cpu_wakeup_latency, Some(250));
        assert_eq!(status.device_resume_latency, Some(5000));
    }

    #[test]
    fn a_status_without_a_class_is_an_error_naming_the_schema() {
        let err = parse_optctl_status(r#"{"schema_version": 3, "decision": {}}"#)
            .expect_err("no class must not parse");
        assert!(err.contains("schema_version 3"), "{err}");
        assert!(err.contains("workload_class"), "{err}");
    }

    #[test]
    fn missing_latency_floors_are_none_not_zero() {
        let status =
            parse_optctl_status(r#"{"schema_version": 2, "decision": {"workload_class": "idle"}}"#)
                .expect("class alone is enough");
        assert_eq!(status.workload_class, "idle");
        assert_eq!(status.cpu_wakeup_latency, None);
        assert_eq!(status.device_resume_latency, None);
    }

    #[test]
    fn non_json_is_rejected() {
        assert!(parse_optctl_status("optid has not written status yet").is_err());
    }
}
