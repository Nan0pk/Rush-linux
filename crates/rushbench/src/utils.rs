use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

pub fn get_sysfs_root() -> PathBuf {
    match env::var("RUSHBENCH_SYSFS_ROOT") {
        Ok(v) => PathBuf::from(v),
        Err(_) => PathBuf::from("/"),
    }
}

pub fn get_kernel_version() -> String {
    if let Ok(v) = env::var("RUSHBENCH_KERNEL_VERSION") {
        return v;
    }
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string()
}

pub fn get_cpu_model() -> String {
    if let Ok(v) = env::var("RUSHBENCH_CPU_MODEL") {
        return v;
    }
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        for line in content.lines() {
            if line.starts_with("model name") {
                if let Some((_, val)) = line.split_once(':') {
                    return val.trim().to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

pub fn get_dmi_board() -> String {
    if let Ok(v) = env::var("RUSHBENCH_DMI_BOARD") {
        return v;
    }
    if let Ok(name) = fs::read_to_string("/sys/class/dmi/id/board_name") {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    if let Ok(name) = fs::read_to_string("/sys/class/dmi/id/product_name") {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    "unknown".to_string()
}

pub fn get_battery_design_uwh() -> u64 {
    if let Ok(v) = env::var("RUSHBENCH_BATTERY_DESIGN_UWH") {
        return v.parse().unwrap_or(0);
    }
    let sysfs_root = get_sysfs_root();
    let power_supply = sysfs_root.join("sys/class/power_supply");
    if let Ok(entries) = fs::read_dir(power_supply) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("BAT") {
                if let Ok(content) = fs::read_to_string(entry.path().join("energy_full_design")) {
                    if let Ok(val) = content.trim().parse::<u64>() {
                        return val;
                    }
                }
                if let Ok(content) = fs::read_to_string(entry.path().join("charge_full_design")) {
                    if let Ok(charge) = content.trim().parse::<u64>() {
                        if let Ok(volt_content) =
                            fs::read_to_string(entry.path().join("voltage_min_design"))
                        {
                            if let Ok(volt) = volt_content.trim().parse::<u64>() {
                                return charge * volt / 1_000_000;
                            }
                        }
                        return charge;
                    }
                }
            }
        }
    }
    0
}

pub fn get_git_sha() -> io::Result<String> {
    if let Ok(sha) = env::var("RUSHBENCH_GIT_SHA") {
        return Ok(sha);
    }
    let output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(io::Error::other("git rev-parse failed"))
    }
}

pub fn get_contracts_sha256() -> io::Result<String> {
    if let Ok(sha) = env::var("RUSHBENCH_CONTRACTS_SHA256") {
        return Ok(sha);
    }
    let contracts_path = find_repo_file("config/optid/contracts.toml")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "contracts.toml not found"))?;
    let output = std::process::Command::new("sha256sum")
        .arg(&contracts_path)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(sha) = stdout.split_whitespace().next() {
        Ok(sha.to_string())
    } else {
        Err(io::Error::other("Failed to parse sha256sum output"))
    }
}

pub fn find_repo_file(relative_path: &str) -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;
    loop {
        let candidate = current.join(relative_path);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn get_utc_timestamp() -> String {
    if let Ok(ts) = env::var("RUSHBENCH_MOCK_TIMESTAMP") {
        return ts;
    }
    let output = std::process::Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!(
                "2026-06-14T{:02}:{:02}:{:02}Z",
                (secs / 3600) % 24,
                (secs / 60) % 60,
                secs % 60
            )
        }
    }
}

pub fn get_host_folder_name() -> String {
    if let Ok(f) = env::var("RUSHBENCH_MOCK_HOST_FOLDER") {
        return f;
    }
    let hostname = fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();
    hostname.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
}

pub fn percentile(sorted: &[u64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = pct * (sorted.len() - 1) as f64;
    let low = idx.floor() as usize;
    let high = idx.ceil() as usize;
    if low == high {
        sorted[low] as f64
    } else {
        let weight = idx - low as f64;
        (sorted[low] as f64) * (1.0 - weight) + (sorted[high] as f64) * weight
    }
}
