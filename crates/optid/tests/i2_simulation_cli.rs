#![cfg(feature = "test-simulation")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};

fn optid_simulation() -> Command {
    Command::new(env!("CARGO_BIN_EXE_optid-simulation"))
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/i2-simulation")
}

#[cfg(unix)]
fn unique_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("optid-i2-{name}-{}-{nonce}", std::process::id()))
}

fn file_inventory(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut inventory = fs::read_dir(root)
        .expect("read fixture root")
        .map(|entry| {
            let entry = entry.expect("fixture entry");
            let name = entry.file_name().into_string().expect("UTF-8 fixture name");
            let bytes = fs::read(entry.path()).expect("read fixture file");
            (name, bytes)
        })
        .collect::<Vec<_>>();
    inventory.sort_by(|left, right| left.0.cmp(&right.0));
    inventory
}

#[test]
fn i2_production_cli_runs_the_complete_simulation_matrix() {
    let root = fixture_root();
    let before = file_inventory(&root);
    let output = optid_simulation()
        .args(["--simulation-root", root.to_str().expect("UTF-8 root")])
        .output()
        .expect("run production simulation CLI entry point");
    assert!(
        output.status.success(),
        "simulation CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("typed matrix report");
    assert_eq!(report["test_only"], true);
    assert_eq!(report["matrix_passed"], true);
    assert_eq!(report["host_write_attempts"], 0);
    assert_eq!(report["reproduction_fixture"]["reproduced"], true);
    for mode in [
        "off",
        "observe",
        "individual_actuation",
        "combined_actuation",
    ] {
        assert!(
            report["scenario_results"]
                .as_array()
                .expect("scenario result array")
                .iter()
                .any(|result| result["mode"] == mode),
            "production CLI omitted {mode}"
        );
    }
    assert_eq!(
        before,
        file_inventory(&root),
        "simulation modified its fixture root"
    );
}

#[test]
fn i2_simulation_rejects_real_root_and_mutation_flags() {
    let root_output = optid_simulation()
        .args(["--simulation-root", "/"])
        .output()
        .expect("run root rejection");
    assert!(!root_output.status.success());
    assert!(String::from_utf8_lossy(&root_output.stderr).contains("dedicated non-root"));

    let fixture = fixture_root();
    let apply_output = optid_simulation()
        .args([
            "--simulation-root",
            fixture.to_str().expect("UTF-8 root"),
            "--apply",
        ])
        .output()
        .expect("run mutation-flag rejection");
    assert!(!apply_output.status.success());
    assert!(String::from_utf8_lossy(&apply_output.stderr).contains("cannot be combined"));
}

#[cfg(unix)]
#[test]
fn i2_simulation_rejects_symlink_roots_and_inputs() {
    use std::os::unix::fs::symlink;

    let symlink_root = unique_root("root-symlink");
    symlink(fixture_root(), &symlink_root).expect("create root symlink");
    let output = optid_simulation()
        .args([
            "--simulation-root",
            symlink_root.to_str().expect("UTF-8 root"),
        ])
        .output()
        .expect("run symlink-root rejection");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a symlink"));
    fs::remove_file(&symlink_root).expect("remove root symlink");

    let input_root = unique_root("input-symlink");
    fs::create_dir(&input_root).expect("create isolated input root");
    fs::write(
        input_root.join(".optid-simulation-root-v1"),
        "optid-test-simulation-root-v1\n",
    )
    .expect("write root marker");
    symlink(
        fixture_root().join("full-system-matrix-v1.json"),
        input_root.join("full-system-matrix-v1.json"),
    )
    .expect("create manifest symlink");
    let output = optid_simulation()
        .args([
            "--simulation-root",
            input_root.to_str().expect("UTF-8 root"),
        ])
        .output()
        .expect("run symlink-input rejection");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-symlink file"));
    fs::remove_file(input_root.join("full-system-matrix-v1.json"))
        .expect("remove manifest symlink");
    fs::remove_file(input_root.join(".optid-simulation-root-v1")).expect("remove marker");
    fs::remove_dir(&input_root).expect("remove isolated input root");
}
