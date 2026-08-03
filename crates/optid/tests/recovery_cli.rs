use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn temp_root(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("rush-s3d-cli-{name}-{}-{id}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create S3D CLI root");
    path
}

fn write_record(root: &Path, target: &Path) {
    let canonical = fs::canonicalize(target).expect("canonical target");
    let record = json!({
        "schema_version": 1,
        "generation": "crashed-generation",
        "owner": "optid",
        "domain": "vm",
        "operation": "vm_sysctl",
        "target_id": "vm-sysctl:cli",
        "canonical_identity": format!("kernel:{}", canonical.display()),
        "target": {"kind": "kernel_value", "path": target},
        "original": {"kind": "scalar", "value": "original"},
        "intended": {"kind": "scalar", "value": "intended"},
        "rollback_method": "restore captured original",
        "stabilization_method": "none",
        "phase": "committed",
        "created_at_unix": 1,
        "updated_at_unix": 1
    });
    fs::write(
        root.join("cli-record.json"),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();
}

#[test]
fn s3d_recovery_cli_recovers_before_success_exit() {
    let root = temp_root("success");
    let target = root.join("target");
    let status = root.join("status.json");
    fs::write(&target, "intended\n").unwrap();
    write_record(&root, &target);

    let output = Command::new(env!("CARGO_BIN_EXE_optid-recover"))
        .args([
            "--recovery-dir",
            root.to_str().unwrap(),
            "--status-file",
            status.to_str().unwrap(),
        ])
        .output()
        .expect("run optid-recover");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&target).unwrap().trim(), "original");
    assert!(status.exists());
    assert!(!root.join("cli-record.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s3d_recovery_binary_has_no_policy_or_async_surface() {
    let source = include_str!("../src/bin/optid-recover.rs");
    let recovery = include_str!("../src/recovery.rs");
    for forbidden in ["Policy", "classif", "zbus", "tokio", "D-Bus", "session"] {
        assert!(
            !source.contains(forbidden),
            "recovery binary contains forbidden surface {forbidden}"
        );
    }
    for forbidden in ["zbus", "tokio"] {
        assert!(
            !recovery.contains(forbidden),
            "recovery core contains forbidden dependency {forbidden}"
        );
    }
}
