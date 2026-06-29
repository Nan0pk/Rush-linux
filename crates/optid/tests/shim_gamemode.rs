//! v0.6 Phase B2 — source-level smoke tests for the GameMode shim.
//!
//! `optid` is a binary crate (no library target), so integration tests
//! in this directory cannot import `crate::shim::gamemode::GameModeServer`
//! directly. The functional tests live inline in
//! `crates/optid/src/shim/gamemode.rs`'s `#[cfg(test)] mod tests` block.
//!
//! This file performs source-level smoke tests using `include_str!` to
//! verify the GameMode shim's interface is declared correctly.

#![allow(dead_code)]

const GAMEMODE_RS: &str = include_str!("../src/shim/gamemode.rs");
// The default TTL / pin_class functions live in policy.rs alongside the
// GameModeShimConfig struct, not in gamemode.rs.
const POLICY_RS: &str = include_str!("../src/policy.rs");

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn gamemode_interface_name_is_declared_on_impl_block() {
    assert!(
        GAMEMODE_RS.contains(r#"#[interface(name = "com.feralinteractive.GameMode")]"#),
        "GameMode interface name must be com.feralinteractive.GameMode"
    );
}

#[test]
fn gamemode_struct_is_named_gamemodeserver() {
    assert!(
        GAMEMODE_RS.contains("pub(crate) struct GameModeServer {"),
        "GameMode shim struct must be named GameModeServer"
    );
}

#[test]
fn gamemode_constructor_takes_state_dir_pin_class_and_ttl() {
    assert!(
        GAMEMODE_RS.contains(
            "pub(crate) fn new(state_dir: PathBuf, pin_class: String, ttl_sec: u64) -> Self"
        ),
        "GameModeServer::new signature changed — update main.rs wiring"
    );
}

#[test]
fn gamemode_register_game_method_exists() {
    assert!(
        GAMEMODE_RS.contains("fn register_game(&self, pid: i32) -> i32"),
        "RegisterGame method must exist with the pinned signature"
    );
}

#[test]
fn gamemode_unregister_game_method_exists() {
    assert!(
        GAMEMODE_RS.contains("fn unregister_game(&self, pid: i32) -> i32"),
        "UnregisterGame method must exist with the pinned signature"
    );
}

#[test]
fn gamemode_query_status_method_exists() {
    assert!(
        GAMEMODE_RS.contains("fn query_status(&self) -> i32"),
        "QueryStatus method must exist with the pinned signature"
    );
}

#[test]
fn gamemode_query_status_client_method_exists() {
    assert!(
        GAMEMODE_RS.contains("fn query_status_client(&self, pid: i32) -> i32"),
        "QueryStatusClient method must exist with the pinned signature"
    );
}

#[test]
fn gamemode_no_properties_or_signals_declared() {
    assert_eq!(
        count(GAMEMODE_RS, "#[zbus(property"),
        0,
        "GameMode shim must not declare any D-Bus properties"
    );
    assert_eq!(
        count(GAMEMODE_RS, "#[zbus(signal"),
        0,
        "GameMode shim must not declare any D-Bus signals"
    );
}

#[test]
fn gamemode_default_ttl_is_30_minutes() {
    assert!(
        POLICY_RS.contains("fn default_gamemode_ttl_sec() -> u64 {"),
        "default_gamemode_ttl_sec function must exist in policy.rs"
    );
    let ttl_fn_start = POLICY_RS
        .find("fn default_gamemode_ttl_sec() -> u64 {")
        .expect("default_gamemode_ttl_sec function must exist in policy.rs");
    let ttl_fn_end = ttl_fn_start
        + POLICY_RS[ttl_fn_start..]
            .find("}")
            .expect("default_gamemode_ttl_sec must close with }}");
    let ttl_fn_body = &POLICY_RS[ttl_fn_start..ttl_fn_end];
    assert!(
        ttl_fn_body.contains("1800"),
        "Default GameMode TTL must be 1800 seconds — function body: {ttl_fn_body}"
    );
}

#[test]
fn gamemode_default_pin_class_is_latency_critical() {
    assert!(
        POLICY_RS.contains("fn default_gamemode_pin_class() -> String {"),
        "default_gamemode_pin_class function must exist in policy.rs"
    );
    let pin_fn_start = POLICY_RS
        .find("fn default_gamemode_pin_class() -> String {")
        .expect("default_gamemode_pin_class function must exist in policy.rs");
    let pin_fn_end = pin_fn_start
        + POLICY_RS[pin_fn_start..]
            .find("}")
            .expect("default_gamemode_pin_class must close with }}");
    let pin_fn_body = &POLICY_RS[pin_fn_start..pin_fn_end];
    assert!(
        pin_fn_body.contains("\"latency-critical\""),
        "Default GameMode pin_class must be latency-critical — function body: {pin_fn_body}"
    );
}

#[test]
fn gamemode_negative_pid_rejected() {
    assert!(
        GAMEMODE_RS.contains("if pid < 0 {\n            return 0;\n        }"),
        "RegisterGame must reject negative PIDs with return value 0"
    );
}

#[test]
fn gamemode_registry_uses_mutex_for_interior_mutability() {
    assert!(
        GAMEMODE_RS.contains("registry: Mutex<GameRegistry>"),
        "registry field must be Mutex<GameRegistry>"
    );
}

#[test]
fn gamemode_pin_class_is_valid_method_exists() {
    assert!(
        GAMEMODE_RS.contains("pub(crate) fn pin_class_is_valid(&self) -> bool"),
        "pin_class_is_valid method must exist (used by main.rs startup check)"
    );
}

#[test]
fn gamemode_module_docstring_mentions_adr_0004() {
    assert!(
        GAMEMODE_RS.contains("ADR 0004"),
        "Module docstring must reference ADR 0004 (optid is single policy owner)"
    );
}

#[test]
fn gamemode_writes_to_state_dir_pins_subdir() {
    assert!(
        GAMEMODE_RS.contains("self.state_dir.join(\"pins\")"),
        "GameMode shim must write pin files to state_dir/pins/"
    );
    assert!(
        GAMEMODE_RS.contains("pins_dir.join(pid.to_string())"),
        "GameMode shim must use the PID as the pin filename"
    );
}

#[test]
fn gamemode_lazy_expiration_is_documented() {
    assert!(
        GAMEMODE_RS.contains("lazy"),
        "Module must document the lazy-expiration policy"
    );
}
