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

#[test]
fn recovery_relinquished_records_never_touch_removed_or_replaced_targets() {
    for replacement in [false, true] {
        let root = temp_root("relinquished");
        let records = root.join("records");
        let target = root.join("device");
        let status = root.join("status.json");
        fs::create_dir_all(&records).unwrap();
        fs::write(&target, "intended\n").unwrap();
        write_record(&records, &target);
        let record_path = records.join("cli-record.json");
        let mut record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&record_path).unwrap()).unwrap();
        record["phase"] = json!("relinquished");
        fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
        fs::remove_file(&target).unwrap();
        if replacement {
            // Matching the old intended value must not transfer ownership of
            // this new device back to the old transaction.
            fs::write(&target, "intended\n").unwrap();
        }

        for expected_scanned in [1, 0] {
            let output = Command::new(env!("CARGO_BIN_EXE_optid-recover"))
                .arg("--recovery-dir")
                .arg(&records)
                .arg("--status-file")
                .arg(&status)
                .output()
                .expect("run standalone recovery");
            assert!(output.status.success(), "{output:?}");
            let summary: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&status).unwrap()).unwrap();
            assert_eq!(summary["scanned"], expected_scanned);
            assert_eq!(summary["relinquished"], expected_scanned);
            assert_eq!(summary["restored"], 0);
            assert_eq!(summary["failed"], 0);
            assert!(!record_path.exists());
            if replacement {
                assert_eq!(fs::read_to_string(&target).unwrap(), "intended\n");
            } else {
                assert!(!target.exists());
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn recovery_retains_unresolved_or_invalid_records_for_missing_targets() {
    for (field, value) in [
        ("phase", json!("committed")),
        ("owner", json!("another-owner")),
        ("schema_version", json!(999)),
    ] {
        let root = temp_root("missing-unresolved");
        let records = root.join("records");
        let target = root.join("device");
        fs::create_dir_all(&records).unwrap();
        fs::write(&target, "intended\n").unwrap();
        write_record(&records, &target);
        let record_path = records.join("cli-record.json");
        let mut record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&record_path).unwrap()).unwrap();
        record["phase"] = json!("relinquished");
        record[field] = value;
        fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
        fs::remove_file(&target).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_optid-recover"))
            .arg("--recovery-dir")
            .arg(&records)
            .arg("--status-file")
            .arg(root.join("status.json"))
            .output()
            .expect("run standalone recovery");
        assert!(!output.status.success(), "{field}: {output:?}");
        let summary: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("recovery failure summary");
        assert_eq!(summary["failed"], 1);
        assert!(record_path.exists(), "{field}: unresolved evidence lost");
        assert!(!target.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
