use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_output() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rushbench-pair-plan-{}-{nonce}/nested/plan.json",
        std::process::id()
    ))
}

#[test]
fn pair_plan_cli_writes_reproducible_balanced_plan() {
    let output_path = temp_output();
    let result = Command::new(env!("CARGO_BIN_EXE_rushbench"))
        .args(["pair-plan", "--pairs", "6", "--seed", "99", "--out"])
        .arg(&output_path)
        .output()
        .expect("run rushbench pair-plan");
    assert!(
        result.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let text = fs::read_to_string(&output_path).expect("pair plan written");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid pair plan JSON");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["seed"], 99);
    assert_eq!(value["pairs"], 6);
    let order = value["order"].as_array().expect("order array");
    assert_eq!(order.len(), 6);
    assert_eq!(
        order.iter().filter(|entry| entry.as_str() == Some("AB")).count(),
        3
    );
    assert_eq!(
        order.iter().filter(|entry| entry.as_str() == Some("BA")).count(),
        3
    );

    let parent = output_path.parent().unwrap().parent().unwrap();
    fs::remove_dir_all(parent).expect("remove pair-plan fixture");
}

#[test]
fn pair_plan_cli_rejects_zero_pairs() {
    let result = Command::new(env!("CARGO_BIN_EXE_rushbench"))
        .args(["pair-plan", "--pairs", "0", "--seed", "1"])
        .output()
        .expect("run rushbench pair-plan");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--pairs must be at least 1"));
}
