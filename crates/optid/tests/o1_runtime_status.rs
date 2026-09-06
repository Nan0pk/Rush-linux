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
    assert!(report.contains("sources.wakeup="));
    assert!(report.contains("cpu_idle="));
    assert!(report.contains("backlight="));
}

#[test]
fn o1_production_off_mode_reports_zero_runtime_reads() {
    let report = observe("off", &[]);
    assert!(report.contains("observability.runtime=off status=disabled reads=0"));
}

#[test]
fn o1_production_pm_qos_preserves_kernel_read_errors() {
    let path = "/sys/kernel/debug/pm_qos/cpu_latency_constraints";
    let before = fs::read_to_string(path).err().map(|error| error.kind());
    let report = observe("observe", &[]);
    let after = fs::read_to_string(path).err().map(|error| error.kind());
    // Compare only a stable error across the sampling window. A readable
    // debugfs surface or a concurrent mount change has no error to assert.
    if before != after {
        return;
    }
    let expected = match before {
        Some(std::io::ErrorKind::PermissionDenied) => "permission_denied",
        Some(std::io::ErrorKind::NotFound) => "unsupported",
        Some(_) => "malformed",
        None => return,
    };
    let line = report
        .lines()
        .find(|line| line.starts_with("pm_qos.cpu_latency_us="))
        .expect("production report includes PM QoS");
    assert!(
        line.ends_with(&format!("status={expected}")),
        "kernel error {before:?}: {line}"
    );
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

/// Both defects O1's first cold verification found were invisible to the
/// module's own fixture and visible in the first snapshot on real hardware: a
/// wakeup file name the kernel does not export, and a documented
/// `runtime_status` value treated as corruption. Neither can be pinned by a
/// test that supplies its own sysfs, so pin them here, on the production
/// binary, against whatever kernel runs the suite.
#[test]
fn o1_production_reports_real_kernel_surfaces_without_degrading_them() {
    let report = observe("observe", &["--samples", "2", "--interval-seconds", "1"]);
    let mut samples: Vec<Vec<&str>> = Vec::new();
    for line in report.lines() {
        if line.starts_with("observability.runtime=") {
            samples.push(Vec::new());
        }
        if let Some(current) = samples.last_mut() {
            current.push(line);
        }
    }
    assert_eq!(samples.len(), 2, "each sample must report exactly once");
    let second = &samples[1];

    // Finding 1: the reporter read `total_time`, which no kernel exports, so
    // every wakeup source on a working machine degraded to `unsupported` with
    // its deltas suppressed. A source that vanished between the two samples is
    // correctly `stale` and is not evidence of that defect.
    for line in second.iter().filter(|line| line.starts_with("wakeup.")) {
        assert!(
            !line.contains("status=unsupported"),
            "a wakeup source on a live kernel must not report unsupported: {line}"
        );
    }

    // A source present in both samples must produce a real total-time delta.
    // Zero is a valid delta on an idle host; `unavailable` is not, unless the
    // source only appeared for the second sample.
    let first_sources = samples[0]
        .iter()
        .filter(|line| line.starts_with("wakeup."))
        .count();
    if first_sources > 0 {
        assert!(
            second.iter().any(|line| {
                line.starts_with("wakeup.")
                    && line.split_whitespace().any(|field| {
                        matches!(
                            field.strip_prefix("total_time_delta_us="),
                            Some(value) if value.chars().all(|c| c.is_ascii_digit())
                        )
                    })
            }),
            "no wakeup source produced a total-time delta across two samples"
        );
    }

    // Finding 2: `unsupported` is the kernel's documented answer for a device
    // whose driver implements no runtime PM. Reporting it is truthful;
    // reporting it as corrupt data is the failure the package title forbids.
    // Asserted as "no device is malformed" rather than "unsupported devices
    // are not applicable", because the pre-repair code dropped the value it
    // rejected — so a test that only inspects lines already reporting
    // `unsupported` would pass against the defect it exists to catch. A real
    // malformed device would fail this, which is the correct outcome: it is a
    // finding, not a flake.
    for line in second.iter().filter(|line| line.starts_with("runtime_pm.")) {
        assert!(
            !line.contains("status=malformed"),
            "a documented kernel value must not be reported as corrupt data: {line}"
        );
        if line.contains("=unsupported ") {
            assert!(
                line.contains("status=not_applicable"),
                "a runtime_status of unsupported is not applicable, not observed: {line}"
            );
        }
    }
}
