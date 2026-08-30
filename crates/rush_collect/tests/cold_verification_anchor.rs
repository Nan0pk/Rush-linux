#[test]
fn cold_verification_anchor() {
    // Temporary verifier-only anchor: selects the full Rust CI lane without
    // changing any optid production, test, policy, packaging, or proof path.
    let cwd = std::env::current_dir().expect("CI has a current directory");
    assert!(cwd.is_absolute());
}
