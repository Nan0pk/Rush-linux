use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_state(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock must be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "optid_o1_{label}_{}_{}",
        std::process::id(),
        nonce
    ))
}

fn run_once(mode: &str) -> String {
    let state_dir = isolated_state(mode);
    fs::create_dir_all(&state_dir).expect("create isolated O1 state directory");
    let config_path = state_dir.join("policy.toml");
    fs::write(
        &config_path,
        format!("[observability.runtime]\nmode = \"{mode}\"\n"),
    )
    .expect("write O1 policy fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_optid"))
        .arg("--once")
        .arg("--no-allowlist")
        .arg("--foreground=off")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("run production optid binary");

    assert!(
        output.status.success(),
        "optid --once failed: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let status = fs::read_to_string(state_dir.join("status"))
        .expect("production optid must write human status");
    fs::remove_dir_all(&state_dir).expect("remove isolated O1 state directory");
    status
}

#[test]
fn o1_production_status_surfaces_runtime_observability() {
    let status = run_once("observe");
    assert!(status.contains("observability.runtime=observe"));
    assert!(status.contains("reads="));
    assert!(status.contains("runtime_pm="));
    assert!(status.contains("cpu_idle="));
    assert!(status.contains("storage="));
    assert!(status.contains("backlights="));
}

#[test]
fn o1_production_off_mode_reports_zero_runtime_reads() {
    let status = run_once("off");
    assert!(status.contains("observability.runtime=off status=disabled reads=0"));
}
