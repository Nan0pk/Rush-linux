use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

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
