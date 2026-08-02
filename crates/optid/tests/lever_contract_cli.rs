use std::process::Command;

#[test]
fn s1d_contract_cli_checks_registry_through_production_surface() {
    let binary = env!("CARGO_BIN_EXE_optid-lever-contracts");

    let check = Command::new(binary)
        .arg("--check")
        .output()
        .expect("run S1D checker");
    assert!(check.status.success(), "{check:?}");
    assert_eq!(
        String::from_utf8(check.stdout).expect("utf-8 checker output"),
        "S1D lever contracts valid: 11\n"
    );

    let json = Command::new(binary)
        .arg("--json")
        .output()
        .expect("run S1D JSON listing");
    assert!(json.status.success(), "{json:?}");
    let rows: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("S1D JSON output parses");
    assert_eq!(rows.as_array().expect("registry JSON array").len(), 11);
}
