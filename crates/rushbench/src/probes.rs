use std::env;
use std::fs;
use std::io;
use std::time::Instant;

pub enum ProbeResult {
    Success(u64),
    UnsupportedHere(String),
    Failed(String),
}

pub fn run_probe_for_metric(metric: &str) -> ProbeResult {
    if let Ok(mock_val) = env::var(format!(
        "RUSHBENCH_MOCK_METRIC_{}",
        metric.replace('-', "_")
    )) {
        if mock_val == "unsupported_here" {
            return ProbeResult::UnsupportedHere("mock unsupported".to_string());
        }
        if let Some(stripped) = mock_val.strip_prefix("failed:") {
            return ProbeResult::Failed(stripped.to_string());
        }
        if let Ok(val) = mock_val.parse::<u64>() {
            return ProbeResult::Success(val);
        }
    }

    match metric {
        "foreground-launch-ms" => {
            if env::var("DISPLAY").is_err() && env::var("WAYLAND_DISPLAY").is_err() {
                return ProbeResult::UnsupportedHere("headless environment".to_string());
            }
            let start = Instant::now();
            let spawn_res = std::process::Command::new("xterm")
                .arg("-e")
                .arg("true")
                .spawn();
            match spawn_res {
                Ok(mut child) => match child.wait() {
                    Ok(status) => {
                        if status.success() {
                            ProbeResult::Success(start.elapsed().as_millis() as u64)
                        } else {
                            ProbeResult::Failed("xterm exited with error".to_string())
                        }
                    }
                    Err(e) => ProbeResult::Failed(format!("failed to wait for xterm: {e}")),
                },
                Err(e) => ProbeResult::UnsupportedHere(format!("xterm not available: {e}")),
            }
        }
        "cyclictest-max-us" => {
            let res = std::process::Command::new("sudo")
                .arg("cyclictest")
                .arg("-l")
                .arg("1000")
                .arg("-q")
                .output();
            match res {
                Ok(out) => {
                    if out.status.success() {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if let Some(max_val) = parse_cyclictest_max(&stdout) {
                            ProbeResult::Success(max_val)
                        } else {
                            ProbeResult::Failed("Failed to parse cyclictest output".to_string())
                        }
                    } else {
                        ProbeResult::Failed(format!(
                            "cyclictest failed: {}",
                            String::from_utf8_lossy(&out.stderr)
                        ))
                    }
                }
                Err(_) => {
                    ProbeResult::UnsupportedHere("cyclictest or sudo not available".to_string())
                }
            }
        }
        "input-latency-p95-ms" | "input-latency-p99-ms" => ProbeResult::UnsupportedHere(
            "evemu-style testing requires local graphical session and evemu tool".to_string(),
        ),
        "psi-cpu-avg10" => match read_psi_avg10("/proc/pressure/cpu") {
            Ok(val) => ProbeResult::Success((val * 1000.0) as u64),
            Err(e) => ProbeResult::Failed(format!("failed to read /proc/pressure/cpu: {e}")),
        },
        "psi-io-avg10" => match read_psi_avg10("/proc/pressure/io") {
            Ok(val) => ProbeResult::Success((val * 1000.0) as u64),
            Err(e) => ProbeResult::Failed(format!("failed to read /proc/pressure/io: {e}")),
        },
        _ => ProbeResult::Failed(format!("unknown metric: {metric}")),
    }
}

pub fn read_psi_avg10(path: &str) -> io::Result<f64> {
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        if line.starts_with("some ") {
            for part in line.split_whitespace() {
                if let Some(stripped) = part.strip_prefix("avg10=") {
                    if let Ok(val) = stripped.parse::<f64>() {
                        return Ok(val);
                    }
                }
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "avg10 not found"))
}

pub fn parse_cyclictest_max(output: &str) -> Option<u64> {
    for line in output.lines() {
        if let Some(idx) = line.find("Max:") {
            let sub = &line[idx + 4..];
            let max_str: String = sub
                .chars()
                .take_while(|c| c.is_ascii_digit() || c.is_whitespace())
                .collect();
            if let Ok(val) = max_str.trim().parse::<u64>() {
                return Some(val);
            }
        }
    }
    None
}
