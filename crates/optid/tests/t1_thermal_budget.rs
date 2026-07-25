//! T1 package-completion — production-surface and contract integration check.
//!
//! The T1 plan (`OPTID-COMPLETION-PLAN.md` §4, package T1) and Research Brief 0013 require:
//!   - Read-only thermal sensor discovery (`hwmon` & `thermal_zone`)
//!   - Fan RPM discovery (`hwmon` & `thinkpad ibm/fan`)
//!   - Pure deterministic `ThermalBudget` calculation with linear derating & hysteresis
//!   - Full integration into `Snapshot` and `Domain::Thermal`
//!
//! This integration test verifies that:
//! 1. Thermal module source files contain all required types and logic (`ThermalSensor`, `FanSensor`, `ThermalBudget`, `compute_thermal_budget`).
//! 2. `Snapshot` in `sensors.rs` carries thermal observation fields.
//! 3. `Domain::Thermal` is registered in `policy.rs` and defaults to `DomainMode::Observe`.

const THERMAL_RS: &str = include_str!("../src/thermal.rs");
const SENSORS_RS: &str = include_str!("../src/sensors.rs");
const POLICY_RS: &str = include_str!("../src/policy.rs");
const MAIN_RS: &str = include_str!("../src/main.rs");

#[test]
fn t1_thermal_module_exports_required_structures() {
    assert!(
        THERMAL_RS.contains("pub(crate) struct ThermalSensor"),
        "thermal.rs must define ThermalSensor"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) struct FanSensor"),
        "thermal.rs must define FanSensor"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) struct ThermalConfig"),
        "thermal.rs must define ThermalConfig"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) enum ThermalBudgetState"),
        "thermal.rs must define ThermalBudgetState"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) struct ThermalBudget"),
        "thermal.rs must define ThermalBudget"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) fn compute_thermal_budget"),
        "thermal.rs must define compute_thermal_budget"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) fn discover_thermal_sensors_with"),
        "thermal.rs must define discover_thermal_sensors_with"
    );
    assert!(
        THERMAL_RS.contains("pub(crate) fn discover_fan_sensors_with"),
        "thermal.rs must define discover_fan_sensors_with"
    );
}

#[test]
fn t1_snapshot_integrates_thermal_observations() {
    assert!(
        SENSORS_RS.contains("thermal_sensors"),
        "sensors.rs Snapshot must contain thermal_sensors field"
    );
    assert!(
        SENSORS_RS.contains("fan_sensors"),
        "sensors.rs Snapshot must contain fan_sensors field"
    );
    assert!(
        SENSORS_RS.contains("thermal_budget"),
        "sensors.rs Snapshot must contain thermal_budget field"
    );
    assert!(
        SENSORS_RS.contains("discover_thermal_sensors_with"),
        "sensors.rs collect_with must discover thermal sensors"
    );
}

#[test]
fn t1_policy_registers_thermal_domain() {
    assert!(
        POLICY_RS.contains("Domain::Thermal => \"thermal\""),
        "policy.rs must register Domain::Thermal as \"thermal\""
    );
    assert!(
        POLICY_RS.contains("Domain::Thermal => DomainMode::Observe"),
        "policy.rs must default Domain::Thermal to DomainMode::Observe"
    );
}

#[test]
fn t1_main_declares_thermal_module() {
    assert!(
        MAIN_RS.contains("mod thermal;"),
        "main.rs must declare mod thermal;"
    );
}
