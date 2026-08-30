//! I2 — the simulated-evidence harness, entered through its production CLI.
//!
//! These tests drive `optid --evidence-root`, the feature-gated entry point
//! that runs the real control loop against a simulated machine. They assert the
//! properties the evidence itself depends on: containment, determinism, complete
//! four-value receipts, working controls, and refusal of an unsafe root.

#![cfg(feature = "test-simulation")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MARKER: &str = ".optid-evidence-root-v1";
const MARKER_CONTENT: &str = "optid-simulation-evidence-root-v1\n";

fn optid() -> Command {
    Command::new(env!("CARGO_BIN_EXE_optid"))
}

fn unique_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "optid-evidence-{name}-{}-{nonce}",
        std::process::id()
    ))
}

fn marked_root(name: &str) -> PathBuf {
    let root = unique_root(name);
    fs::create_dir_all(&root).expect("create simulation root");
    fs::write(root.join(MARKER), MARKER_CONTENT).expect("write root marker");
    root
}

fn run_matrix(root: &Path) -> serde_json::Value {
    let output = optid()
        .args([
            "--evidence-root",
            root.to_str().expect("UTF-8 root"),
            "--evidence-repeats",
            "2",
        ])
        .output()
        .expect("run the evidence CLI");
    assert!(
        output.status.success(),
        "evidence run reported a blocking failure:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle = fs::read_to_string(root.join("out/evidence-bundle.json"))
        .expect("evidence bundle was written");
    serde_json::from_str(&bundle).expect("bundle is valid JSON")
}

#[test]
fn i2_evidence_matrix_is_contained_deterministic_and_controlled() {
    let root = marked_root("matrix");
    let bundle = run_matrix(&root);

    // Containment: nothing may leave the verified simulation root.
    assert_eq!(
        bundle["total_host_write_attempts"], 0,
        "a write attempt left the simulation root"
    );
    assert!(
        bundle["containment_violations"]
            .as_array()
            .expect("violation array")
            .is_empty(),
        "containment violations were recorded: {}",
        bundle["containment_violations"]
    );

    // Determinism: every arm/scenario group must repeat byte-identically.
    let determinism = &bundle["determinism"];
    assert_eq!(
        determinism["identical_groups"], determinism["compared_groups"],
        "non-deterministic groups: {}",
        determinism["divergent"]
    );
    assert!(
        determinism["compared_groups"]
            .as_u64()
            .expect("group count")
            > 0,
        "no groups were compared"
    );

    // Controls: the result system must see both no change and deliberate harm.
    assert_eq!(
        bundle["controls"]["no_change_control_held"], true,
        "the no-change control moved machine state: {}",
        bundle["controls"]["no_change_violations"]
    );
    assert_eq!(
        bundle["controls"]["harmful_control_detected"], true,
        "the deliberately harmful control was not detected as harmful"
    );

    // The report must be written next to the bundle.
    let report = fs::read_to_string(root.join("out/report.md")).expect("report was written");
    assert!(report.contains("simulated and modelled"));

    fs::remove_dir_all(&root).expect("clean up the simulation root");
}

#[test]
fn i2_evidence_receipts_record_all_four_values_and_never_pass_an_inert_control() {
    let root = marked_root("receipts");
    let bundle = run_matrix(&root);

    let mut checked = 0usize;
    let mut inert = 0usize;
    for trial in bundle["trials"].as_array().expect("trial array") {
        for receipt in trial["receipts"].as_array().expect("receipt array") {
            checked += 1;
            for field in [
                "previous_value",
                "requested_value",
                "read_back_value",
                "restored_value",
            ] {
                assert!(
                    receipt[field].is_string(),
                    "receipt is missing {field}: {receipt}"
                );
            }
            assert!(
                !receipt["requested_value"]
                    .as_str()
                    .expect("requested value")
                    .is_empty(),
                "receipt has an empty requested value: {receipt}"
            );
            if receipt["classification"] == "inert_control" {
                inert += 1;
                assert_eq!(
                    receipt["became_active"], false,
                    "an inert control was reported as an active action: {receipt}"
                );
            }
        }
    }
    assert!(checked > 0, "no receipts were produced");
    assert!(
        inert > 0,
        "the matrix never exercised an inert control, so the unsupported path is unproven"
    );

    // The simulated machine deliberately carries a control the kernel exposes
    // read-only; it must be reported as unsupported rather than as a pass.
    assert!(
        !bundle["safety"]["refused_writes"]
            .as_array()
            .expect("refused array")
            .is_empty(),
        "no write refusal was observed, so the refusal path is unproven"
    );

    fs::remove_dir_all(&root).expect("clean up the simulation root");
}

#[test]
fn i2_evidence_rejects_an_unsafe_or_unmarked_root() {
    for candidate in ["/", "/sys", "/proc"] {
        let output = optid()
            .args(["--evidence-root", candidate])
            .output()
            .expect("run the evidence CLI");
        assert!(
            !output.status.success(),
            "the evidence CLI accepted {candidate}"
        );
    }

    let unmarked = unique_root("unmarked");
    fs::create_dir_all(&unmarked).expect("create unmarked root");
    let output = optid()
        .args(["--evidence-root", unmarked.to_str().expect("UTF-8 root")])
        .output()
        .expect("run the evidence CLI");
    assert!(!output.status.success(), "an unmarked root was accepted");
    assert!(String::from_utf8_lossy(&output.stderr).contains("unmarked"));
    fs::remove_dir_all(&unmarked).expect("clean up");

    let root = marked_root("repeats");
    let output = optid()
        .args([
            "--evidence-root",
            root.to_str().expect("UTF-8 root"),
            "--evidence-repeats",
            "1",
        ])
        .output()
        .expect("run the evidence CLI");
    assert!(
        !output.status.success(),
        "a single repeat cannot prove determinism and must be refused"
    );
    fs::remove_dir_all(&root).expect("clean up");
}

#[cfg(unix)]
#[test]
fn i2_evidence_rejects_a_symlinked_root() {
    use std::os::unix::fs::symlink;

    let real = marked_root("symlink-target");
    let link = unique_root("symlink");
    symlink(&real, &link).expect("create root symlink");
    let output = optid()
        .args(["--evidence-root", link.to_str().expect("UTF-8 root")])
        .output()
        .expect("run the evidence CLI");
    assert!(!output.status.success(), "a symlinked root was accepted");
    assert!(String::from_utf8_lossy(&output.stderr).contains("symlink"));
    fs::remove_file(&link).expect("remove root symlink");
    fs::remove_dir_all(&real).expect("clean up");
}

#[test]
fn i2_a_failed_policy_reload_never_actuates_a_domain_that_is_switched_off() {
    // Regression guard for the finding this harness first surfaced: the run
    // loop re-reads `policy.toml` every cycle, and `Policy::load` answers an
    // unreadable file with `curated_baseline()`, whose per-domain default is
    // `actuate`. Because `apply_armed` was computed once at startup, an
    // operator's `mode = "off"` silently became actuation mid-run — eight
    // kernel controls moved in the run that found it.
    //
    // The matrix injects exactly that: `config_reload_failure_and_recovery`
    // replaces the policy with unparseable TOML under a running daemon. The
    // detector that found the escalation is still live, so if the fix is ever
    // undone the finding reappears and this test fails.
    let root = marked_root("reload");
    let bundle = run_matrix(&root);

    let findings = bundle["findings"].as_array().expect("findings array");
    let escalation = findings
        .iter()
        .find(|finding| finding["id"] == "policy_reload_fallback_escalates_domain_modes");
    assert!(
        escalation.is_none(),
        "a failed policy reload escalated a domain that was switched off: {}",
        escalation
            .map(|finding| finding["evidence"].to_string())
            .unwrap_or_default()
    );

    // The finding above is derived, so assert the underlying fact directly too:
    // an arm that configures every domain `off` or `observe` must never leave
    // an active action behind, in any scenario, including the reload one.
    let mut checked_reload_scenario = false;
    for trial in bundle["trials"].as_array().expect("trial array") {
        let arm = trial["arm"].as_str().unwrap_or_default();
        if arm != "off_all_domains" && arm != "full_observe" {
            continue;
        }
        if trial["scenario"] == "config_reload_failure_and_recovery" {
            checked_reload_scenario = true;
        }
        for receipt in trial["receipts"].as_array().expect("receipt array") {
            assert_eq!(
                receipt["became_active"], false,
                "{arm} / {} actuated {} while every domain was switched off or observe-only",
                trial["scenario"], receipt["control_id"]
            );
        }
    }
    assert!(
        checked_reload_scenario,
        "the matrix no longer exercises a failed policy reload, so this guard proves nothing"
    );

    fs::remove_dir_all(&root).expect("clean up the simulation root");
}
