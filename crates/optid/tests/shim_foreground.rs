//! v0.6 Phase C1 — source-level smoke tests for the foreground-detection module.

#![allow(dead_code)]

const FOREGROUND_RS: &str = include_str!("../src/foreground/mod.rs");
const ARGS_RS: &str = include_str!("../src/args.rs");

#[test]
fn foreground_module_has_subscribe_function() {
    assert!(
        FOREGROUND_RS.contains("pub(crate) fn subscribe("),
        "foreground module must expose a subscribe() function"
    );
    assert!(
        FOREGROUND_RS.contains("-> mpsc::Receiver<(i32, String)>"),
        "subscribe() must return mpsc::Receiver<(i32, String)>"
    );
}

#[test]
fn foreground_config_struct_exists() {
    assert!(
        FOREGROUND_RS.contains("pub(crate) struct ForegroundConfig"),
        "ForegroundConfig struct must exist"
    );
}

#[test]
fn foreground_config_has_game_class_field() {
    assert!(
        FOREGROUND_RS.contains("pub(crate) game_class: String"),
        "ForegroundConfig must have a game_class: String field"
    );
}

#[test]
fn foreground_config_default_game_class_is_latency_critical() {
    assert!(
        FOREGROUND_RS
            .contains("fn default_game_class() -> String {\n    \"latency-critical\".to_string()"),
        "default_game_class must return latency-critical"
    );
}

#[test]
fn foreground_config_has_manual_default_impl() {
    assert!(
        FOREGROUND_RS.contains("impl Default for ForegroundConfig"),
        "ForegroundConfig must have a manual Default impl"
    );
}

#[test]
fn foreground_module_documents_v0_6_stub_status() {
    assert!(
        FOREGROUND_RS.contains("v0.6 stub") || FOREGROUND_RS.contains("v0.6 **stub**"),
        "Module must document that v0.6 is a stub"
    );
    assert!(
        FOREGROUND_RS.contains("v0.7"),
        "Module must reference v0.7 as the target for real integration"
    );
}

#[test]
fn foreground_module_mentions_login1_and_compositors() {
    assert!(
        FOREGROUND_RS.contains("login1"),
        "Module must mention org.freedesktop.login1 in the v0.7 plan"
    );
    assert!(
        FOREGROUND_RS.contains("Mutter")
            || FOREGROUND_RS.contains("KWin")
            || FOREGROUND_RS.contains("wlr-foreign-toplevel"),
        "Module must mention at least one compositor focus signal in the v0.7 plan"
    );
}

#[test]
fn args_foreground_mode_enum_exists() {
    assert!(
        ARGS_RS.contains("pub(crate) enum ForegroundMode"),
        "ForegroundMode enum must exist in args.rs"
    );
    assert!(
        ARGS_RS.contains("Off,") && ARGS_RS.contains("Auto,"),
        "ForegroundMode must have Off and Auto variants"
    );
}

#[test]
fn args_struct_has_foreground_field() {
    assert!(
        ARGS_RS.contains("pub(crate) foreground: ForegroundMode"),
        "Args struct must have a foreground: ForegroundMode field"
    );
}

#[test]
fn args_foreground_default_is_off() {
    assert!(
        ARGS_RS.contains("foreground: ForegroundMode::Off,"),
        "Args::parse must initialize foreground to ForegroundMode::Off"
    );
}

#[test]
fn args_foreground_accepts_off_and_auto() {
    assert!(
        ARGS_RS.contains("\"--foreground=off\""),
        "Args parser must accept --foreground=off"
    );
    assert!(
        ARGS_RS.contains("\"--foreground=auto\""),
        "Args parser must accept --foreground=auto"
    );
}

#[test]
fn args_foreground_rejects_invalid_values() {
    assert!(
        ARGS_RS.contains("other.starts_with(\"--foreground=\")"),
        "Args parser must catch --foreground=<invalid> with a helpful error"
    );
}

#[test]
fn args_usage_mentions_foreground_flag() {
    assert!(
        ARGS_RS.contains("--foreground=off|auto"),
        "print_usage must mention --foreground=off|auto"
    );
}
