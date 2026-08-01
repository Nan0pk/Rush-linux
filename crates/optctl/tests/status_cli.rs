use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_state_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock must be after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "optctl-f3-cli-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create isolated optctl state directory");
    path
}

fn run_optctl(state_dir: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_optctl"));
    command
        .env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            "unix:path=/definitely-missing-optid-system-bus",
        )
        .arg("--state-dir")
        .arg(state_dir);
    command.args(args);
    command.output().expect("run optctl binary")
}

fn state_snapshot(state_dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(state_dir)
        .expect("read state directory")
        .map(|entry| {
            let entry = entry.expect("read state entry");
            let name = entry
                .file_name()
                .into_string()
                .expect("test state filename is UTF-8");
            let bytes = fs::read(entry.path()).expect("read state file");
            (name, bytes)
        })
        .collect()
}

#[test]
fn f3_optctl_status_json_passthrough_uses_state_fallback_and_is_read_only() {
    let state_dir = temp_state_dir("json-pass-through");
    let golden = include_str!("../../optid/tests/fixtures/f3-control-cycle-v2.json");
    fs::write(state_dir.join("status.json"), golden).expect("write daemon JSON fixture");
    fs::write(state_dir.join("status"), "legacy text must not be parsed\n")
        .expect("write legacy status fixture");
    let before = state_snapshot(&state_dir);

    let output = run_optctl(&state_dir, &["status", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", golden.trim_end())
    );
    assert_eq!(
        state_snapshot(&state_dir),
        before,
        "status inspection must not mutate state"
    );
    fs::remove_dir_all(state_dir).expect("remove isolated state directory");
}

#[test]
fn f3_optctl_status_json_rejects_malformed_and_missing_machine_state() {
    let malformed_dir = temp_state_dir("malformed-json");
    fs::write(malformed_dir.join("status.json"), "{not-json")
        .expect("write malformed status fixture");
    let malformed_before = state_snapshot(&malformed_dir);
    let malformed = run_optctl(&malformed_dir, &["status", "--json"]);
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("malformed daemon status.json"));
    assert_eq!(state_snapshot(&malformed_dir), malformed_before);
    fs::remove_dir_all(malformed_dir).expect("remove malformed state directory");

    let missing_dir = temp_state_dir("missing-json");
    fs::write(missing_dir.join("status"), "mode=balanced\n")
        .expect("write human-only status fixture");
    let missing_before = state_snapshot(&missing_dir);
    let missing = run_optctl(&missing_dir, &["status", "--json"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr)
        .contains("refusing to reconstruct machine status from text"));
    assert_eq!(state_snapshot(&missing_dir), missing_before);
    fs::remove_dir_all(missing_dir).expect("remove missing state directory");
}

#[test]
fn f3_optctl_human_status_remains_backward_compatible_and_read_only() {
    let state_dir = temp_state_dir("human-status");
    let human = "correlation_id=cycle-human\nmode=balanced\nactions:\n";
    fs::write(state_dir.join("status"), human).expect("write human status fixture");
    let before = state_snapshot(&state_dir);

    let output = run_optctl(&state_dir, &["status"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), human);
    assert_eq!(
        state_snapshot(&state_dir),
        before,
        "human status must be read-only"
    );
    fs::remove_dir_all(state_dir).expect("remove isolated state directory");
}
