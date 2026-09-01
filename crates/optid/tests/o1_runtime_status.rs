//! O1 production proof.
//!
//! These tests execute the real compiled `optid-observe` reporter — the
//! package's named production surface — rather than calling the module
//! directly. The reporter is read-only, so it can run against the host in CI
//! without touching kernel or daemon state.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_config(label: &str, mode: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock must be after the Unix epoch")
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("optid_o1_{label}_{}_{}", std::process::id(), nonce));
    fs::create_dir_all(&directory).expect("create isolated O1 fixture directory");
    let config_path = directory.join("policy.toml");
    fs::write(
        &config_path,
        format!("[observability.runtime]\nmode = \"{mode}\"\n"),
    )
    .expect("write O1 policy fixture");
    config_path
}

fn observe(mode: &str, extra: &[&str]) -> String {
    let config_path = isolated_config(mode, mode);

    let output = Command::new(env!("CARGO_BIN_EXE_optid-observe"))
        .arg("--config")
        .arg(&config_path)
        .args(extra)
        .output()
        .expect("run production optid-observe binary");

    assert!(
        output.status.success(),
        "optid-observe failed: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let directory = config_path
        .parent()
        .expect("fixture config always has a parent");
    fs::remove_dir_all(directory).expect("remove isolated O1 fixture directory");

    String::from_utf8(output.stdout).expect("optid-observe must print valid UTF-8")
}

#[test]
fn o1_production_status_surfaces_runtime_observability() {
    let report = observe("observe", &[]);
    assert!(report.contains("observability.runtime=observe"));
    assert!(report.contains("reads="));
    assert!(report.contains("runtime_pm="));
    assert!(report.contains("cpu_idle="));
    assert!(report.contains("storage="));
    assert!(report.contains("backlights="));
}

#[test]
fn o1_production_off_mode_reports_zero_runtime_reads() {
    let report = observe("off", &[]);
    assert!(report.contains("observability.runtime=off status=disabled reads=0"));
}

#[test]
fn o1_production_repeated_sampling_reports_live_state_not_stale() {
    // The reporter takes no state directory and owns no write path; it keeps
    // one previous snapshot in memory. Each sample must report exactly once,
    // and the second must cover real elapsed time rather than being suppressed
    // as stale, which is what the default one-second interval buys.
    let report = observe("observe", &["--samples", "2"]);
    let summaries: Vec<&str> = report
        .lines()
        .filter(|line| line.starts_with("observability.runtime="))
        .collect();
    assert_eq!(summaries.len(), 2, "each sample must report exactly once");
    for summary in &summaries {
        assert!(
            summary.contains("status=observed"),
            "sample must report observed state, got: {summary}"
        );
    }
}
