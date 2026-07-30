#![cfg(feature = "experimental-capability-sealing")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_optid-capability-seal-test")
}

fn unique_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "optid-capability-sealing-{name}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn topology_rebuild_cli_exits_with_dedicated_status() {
    let status = Command::new(binary())
        .arg("--topology-rebuild")
        .status()
        .expect("run topology-rebuild mode");
    assert_eq!(status.code(), Some(75));
}

#[test]
fn recovery_step_cli_writes_a_valid_marker() {
    let marker = unique_path("recovery-marker");
    let status = Command::new(binary())
        .arg("--recovery-step")
        .arg(&marker)
        .status()
        .expect("run recovery-step mode");
    assert!(status.success());

    let contents = fs::read_to_string(&marker).expect("read recovery marker");
    assert!(contents.starts_with("optid-capability-recovery-v1\n"));
    assert!(contents.lines().any(|line| line == "recovery_complete=1"));
    fs::remove_file(marker).expect("remove recovery marker");
}

#[test]
fn service_orders_recovery_before_fresh_capability_construction() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let unit_path = manifest_dir.join("../../packaging/systemd/optid-capability-seal-test.service");
    let unit = fs::read_to_string(&unit_path).expect("read test-only systemd unit");
    let lines: Vec<&str> = unit.lines().collect();

    let recovery = lines
        .iter()
        .position(|line| line.starts_with("ExecStartPre=") && line.contains("--recovery-step"))
        .expect("recovery ExecStartPre");
    let fresh_process = lines
        .iter()
        .position(|line| line.starts_with("ExecStart=") && line.contains("--supervisor-cycle"))
        .expect("supervisor-cycle ExecStart");
    assert!(recovery < fresh_process, "recovery must be declared before ExecStart");

    assert!(unit.contains("RestartForceExitStatus=75"));
    assert!(unit.contains("RuntimeDirectoryPreserve=restart"));
    assert!(unit.contains("NoNewPrivileges=yes"));

    let unit_section = unit
        .split("[Service]")
        .next()
        .expect("unit section before service section");
    assert!(unit_section.contains("StartLimitIntervalSec=60s"));
    assert!(unit_section.contains("StartLimitBurst=5"));
}
