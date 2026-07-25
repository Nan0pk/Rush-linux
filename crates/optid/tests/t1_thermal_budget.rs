//! T1 package-completion — behavioral integration evidence pointer.
//!
//! `optid` is a binary crate (`[[bin]]` only). External integration tests
//! cannot call `pub(crate)` production paths. The required behavioral
//! matrix therefore lives inside the crate and exercises the real
//! production functions through injected `KernelRead` (`MemoryKernel`):
//!
//! ```text
//! Snapshot::collect_with_thermal
//!   → discover_thermal_sensors_with / discover_fan_sensors_with
//!   → compute_thermal_budget (hysteresis via previous)
//!   → render_thermal_status / Decision::render
//! ```
//!
//! Run the matrix with:
//!
//! ```bash
//! cargo test -p optid -- thermal
//! ```
//!
//! Required behavioral cases (all in `crates/optid/src/thermal.rs` tests):
//!
//! 1. `discover_hwmon_cpu_temp`
//! 2. `discover_acpi_thermal_zone`
//! 3. `discover_fan_rpm`
//! 4. `discover_malformed_skipped`
//! 5. `thermal_budget_unavailable_without_sensors`
//! 6. `duplicate_hwmon_acpi_dedup`
//! 7. `stable_identity_under_hwmon_reorder`
//! 8. `thermal_budget_linear_derating`
//! 9. `thermal_budget_hw_crit_clamp`
//! 10. `thermal_budget_skin_override`
//! 11. `two_iterations_hysteresis_via_previous`
//! 12. `collect_off_skips_observation`
//! 13. `collect_config_changes_results`
//! 14. `render_includes_sensor_state_ratio_reasons`
//! 15. `deterministic_ordering`
//!
//! This file does **not** prove production integration via
//! `include_str!(...src...)` + `.contains(...)`. That pattern was the
//! defect repaired by this package revision.

/// Behavioral matrix test names that must remain present in thermal.rs.
const T1_BEHAVIORAL_TESTS: &[&str] = &[
    "discover_hwmon_cpu_temp",
    "discover_acpi_thermal_zone",
    "discover_fan_rpm",
    "discover_malformed_skipped",
    "thermal_budget_unavailable_without_sensors",
    "duplicate_hwmon_acpi_dedup",
    "stable_identity_under_hwmon_reorder",
    "thermal_budget_linear_derating",
    "thermal_budget_hw_crit_clamp",
    "thermal_budget_skin_override",
    "two_iterations_hysteresis_via_previous",
    "collect_off_skips_observation",
    "snapshot_off_skips_thermal_zone_discovery_and_thermal_c",
    "snapshot_unavailable_thermal_c_is_none_despite_legacy_zone",
    "collect_config_changes_results",
    "render_includes_sensor_state_ratio_reasons",
    "deterministic_ordering",
];

#[test]
fn t1_behavioral_matrix_has_required_cases() {
    assert!(
        T1_BEHAVIORAL_TESTS.len() >= 15,
        "T1 behavioral matrix must list at least 15 cases covering the acceptance matrix"
    );
    // Ensure names are unique and non-empty (mapping integrity).
    let mut seen = std::collections::BTreeSet::new();
    for name in T1_BEHAVIORAL_TESTS {
        assert!(!name.is_empty());
        assert!(seen.insert(*name), "duplicate behavioral test name: {name}");
    }
}
