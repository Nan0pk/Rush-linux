//! v0.6 Phase B1 — source-level smoke tests for the PPD shim.
//!
//! `optid` is a binary crate (no library target), so integration tests
//! in this directory cannot import `crate::shim::ppd::PpdServer` directly.
//! The functional tests live inline in `crates/optid/src/shim/ppd.rs`'s
//! `#[cfg(test)] mod tests` block, where they have full access to the
//! module's private items.
//!
//! This file performs source-level smoke tests using `include_str!` to
//! verify the PPD shim's interface is declared correctly. This matches
//! the pattern in `tests/write_site_gating.rs`. It catches:
//!
//! - The `net.hadess.PowerProfiles` interface name is on the impl block.
//! - All required properties are declared: `ActiveProfile`, `Profiles`,
//!   `Actions`, `PerformanceDegraded`.
//! - The `Set` variant of `ActiveProfile` exists.
//! - `HoldProfile` and `ReleaseProfile` methods exist.
//! - The custom signal methods are declared (with the correct D-Bus
//!   member names `ActiveProfileChanged` and `ProfileReleased`).
//! - The default profile → mode mapping is the documented one
//!   (power-saver→battery, balanced→auto, performance→performance).
//!
//! Drift detection: if a future refactor renames or removes any of these
//! symbols, these assertions fail mechanically and force the contributor
//! to update this file (and the inventory in `ppd.rs`).

#![allow(dead_code)]

const PPD_RS: &str = include_str!("../src/shim/ppd.rs");

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn ppd_interface_name_is_declared_on_impl_block() {
    assert!(
        PPD_RS.contains(r#"#[interface(name = "net.hadess.PowerProfiles")]"#),
        "PPD interface name must be net.hadess.PowerProfiles"
    );
}

#[test]
fn ppd_struct_is_named_ppdserver() {
    assert!(
        PPD_RS.contains("pub(crate) struct PpdServer {"),
        "PPD shim struct must be named PpdServer"
    );
}

#[test]
fn ppd_constructor_takes_state_dir_and_profile_map() {
    assert!(
        PPD_RS.contains(
            "pub(crate) fn new(state_dir: PathBuf, profile_map: HashMap<String, String>) -> Self"
        ),
        "PpdServer::new signature changed — update main.rs wiring"
    );
}

#[test]
fn ppd_active_profile_property_is_declared_with_emits_changed_true() {
    assert!(
        PPD_RS.contains(r#"#[zbus(property(emits_changed_signal = "true"))]"#),
        "ActiveProfile getter must have emits_changed_signal = \"true\""
    );
}

#[test]
fn ppd_set_active_profile_setter_is_declared() {
    assert!(
        PPD_RS.contains("fn set_active_profile(&self, profile: String) -> zbus::fdo::Result<()>"),
        "set_active_profile setter must exist with the pinned signature"
    );
}

#[test]
fn ppd_profiles_property_returns_aa_sv() {
    assert!(
        PPD_RS.contains("fn profiles(&self) -> Vec<HashMap<String, OwnedValue>>"),
        "Profiles property must return Vec<HashMap<String, OwnedValue>>"
    );
    assert!(
        PPD_RS.contains("fn profile_entry(name: &str) -> HashMap<String, OwnedValue>"),
        "Profiles property must use a profile_entry helper"
    );
    assert_eq!(
        count(PPD_RS, "profile_entry(\""),
        3,
        "profile_entry helper must be called exactly 3 times (power-saver, balanced, performance)"
    );
}

#[test]
fn ppd_actions_property_returns_empty_vec_string() {
    assert!(
        PPD_RS.contains("fn actions(&self) -> Vec<String>"),
        "Actions property must return Vec<String>"
    );
}

#[test]
fn ppd_performance_degraded_returns_string() {
    assert!(
        PPD_RS.contains("fn performance_degraded(&self) -> String"),
        "PerformanceDegraded property must return String"
    );
}

#[test]
fn ppd_hold_profile_method_exists() {
    assert!(
        PPD_RS.contains("fn hold_profile(\n        &self,\n        profile: String,\n        reason: String,\n        app_id: String,\n    ) -> zbus::fdo::Result<u32>"),
        "HoldProfile method must exist with the pinned signature"
    );
}

#[test]
fn ppd_release_profile_method_exists() {
    assert!(
        PPD_RS.contains("fn release_profile(&self, cookie: u32) -> zbus::fdo::Result<()>"),
        "ReleaseProfile method must exist with the pinned signature"
    );
}

#[test]
fn ppd_active_profile_changed_signal_is_declared() {
    assert!(
        PPD_RS.contains(r#"#[zbus(signal, name = "ActiveProfileChanged")]"#),
        "ActiveProfileChanged signal must be declared with name = \"ActiveProfileChanged\""
    );
    assert!(
        PPD_RS.contains("async fn emit_active_profile_changed("),
        "ActiveProfileChanged signal method must be named emit_active_profile_changed"
    );
}

#[test]
fn ppd_profile_released_signal_is_declared() {
    assert!(
        PPD_RS.contains(r#"#[zbus(signal, name = "ProfileReleased")]"#),
        "ProfileReleased signal must be declared with name = \"ProfileReleased\""
    );
    assert!(
        PPD_RS.contains("async fn emit_profile_released("),
        "ProfileReleased signal method must be named emit_profile_released"
    );
}

#[test]
fn ppd_default_mapping_is_documented_triplet() {
    let mapping_block = "\"power-saver\" => Some(\"battery\"),\n        \"balanced\" => Some(\"auto\"),\n        \"performance\" => Some(\"performance\"),";
    let squash = |s: &str| s.replace([' ', '\n', '\t'], "");
    assert!(
        squash(PPD_RS).contains(&squash(mapping_block)),
        "default_mode_for_profile mapping must match the documented triplet"
    );
}

#[test]
fn ppd_hold_registry_uses_mutex_for_interior_mutability() {
    assert!(
        PPD_RS.contains("holds: Mutex<HoldRegistry>"),
        "holds field must be Mutex<HoldRegistry>"
    );
}

#[test]
fn ppd_first_cookie_is_one_not_zero() {
    assert!(
        PPD_RS.contains("next_cookie: 1,"),
        "HoldRegistry must start cookie counter at 1"
    );
}

#[test]
fn ppd_module_docstring_mentions_adr_0004() {
    assert!(
        PPD_RS.contains("ADR 0004"),
        "Module docstring must reference ADR 0004 (optid is single policy owner)"
    );
}
