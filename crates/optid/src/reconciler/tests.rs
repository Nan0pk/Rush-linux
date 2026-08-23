use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::kernel_io::MemoryKernel;
use crate::load_state::{BootState, LoadState};

#[derive(Clone, Default)]
struct FakeSystemd {
    properties: Rc<RefCell<BTreeMap<(String, String), SystemdPropertyState>>>,
    inherited: Rc<RefCell<BTreeMap<(String, String), String>>>,
    fail_property: Rc<RefCell<Option<String>>>,
    mismatch_property: Rc<RefCell<Option<String>>>,
}

impl FakeSystemd {
    fn seed(&self, unit: &str, property: &str, explicit: bool, value: &str) {
        let key = (unit.to_string(), property.to_string());
        self.properties.borrow_mut().insert(
            key.clone(),
            SystemdPropertyState {
                explicit,
                value: value.to_string(),
            },
        );
        self.inherited.borrow_mut().insert(key, value.to_string());
    }

    fn state(&self, unit: &str, property: &str) -> SystemdPropertyState {
        self.read_property(unit, property).expect("seeded property")
    }
}

impl SystemdIo for FakeSystemd {
    fn read_property(&self, unit: &str, property: &str) -> io::Result<SystemdPropertyState> {
        self.properties
            .borrow()
            .get(&(unit.to_string(), property.to_string()))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "property missing"))
    }

    fn set_property(&self, unit: &str, property: &str, value: Option<&str>) -> io::Result<()> {
        if self.fail_property.borrow().as_deref() == Some(property) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"));
        }
        let key = (unit.to_string(), property.to_string());
        let inherited = self
            .inherited
            .borrow()
            .get(&key)
            .cloned()
            .unwrap_or_else(|| "default".to_string());
        let stored = match value {
            Some(value) if self.mismatch_property.borrow().as_deref() == Some(property) => {
                SystemdPropertyState {
                    explicit: true,
                    value: format!("mismatch-{value}"),
                }
            }
            Some(value) => SystemdPropertyState {
                explicit: true,
                value: value.to_string(),
            },
            None => SystemdPropertyState {
                explicit: false,
                value: inherited,
            },
        };
        self.properties.borrow_mut().insert(key, stored);
        Ok(())
    }
}

fn armed_actuator(state_dir: PathBuf, kernel: MemoryKernel) -> Actuator {
    let mut actuator = Actuator::new_with_kernel(state_dir, Box::new(kernel));
    actuator.set_boot_state(BootState {
        policy_load_state: LoadState::Ok,
        allowlist_load_state: LoadState::Ok,
        apply_armed: true,
        baseline_armed: false,
        allowlist_gate_enabled: false,
    });
    actuator.bypass_contract_gate = true;
    actuator
}

fn runtime_pm_action(device_dir: &Path, delay: i32) -> Action {
    Action::RuntimePm {
        device_dir: device_dir.to_path_buf(),
        autosuspend_delay_ms: delay,
        reason: "F4 production acceptance".to_string(),
    }
}

fn vm_action(path: &Path, value: &str) -> Action {
    Action::VmSysctl {
        path: path.to_path_buf(),
        value: value.to_string(),
        reason: "F4 production acceptance".to_string(),
    }
}

fn systemd_action(properties: &[&str]) -> Action {
    Action::SystemdSetProperty {
        unit: "background.slice".to_string(),
        properties: properties
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        reason: "F4 production acceptance".to_string(),
    }
}

fn state(target_id: &str, desired: Option<&str>) -> TargetState {
    TargetState {
        target_id: target_id.to_string(),
        domain: "runtime_pm".to_string(),
        target: TargetKind::KernelValue {
            path: PathBuf::from(format!("/sys/test/{target_id}/power/control")),
        },
        legacy_journal_key: Some(format!("rpm_{target_id}")),
        baseline: Some(StoredValue::Scalar {
            value: "on".to_string(),
        }),
        desired: desired.map(|value| StoredValue::Scalar {
            value: value.to_string(),
        }),
        last_attempted: Some(StoredValue::Scalar {
            value: "auto".to_string(),
        }),
        last_confirmed: Some(StoredValue::Scalar {
            value: "auto".to_string(),
        }),
        ownership: OwnershipState::Optid,
        ownership_reason: None,
        retries: 0,
        restore_pending: true,
    }
}

fn reconciler_with(targets: BTreeMap<String, TargetState>, mode: ReconcileMode) -> Reconciler {
    Reconciler {
        state_dir: PathBuf::from("/run/optid"),
        mode,
        targets,
        previous_desired: BTreeSet::new(),
        last_ac: None,
        last_workload: WorkloadClass::Idle,
        last_mode: Mode::Auto,
        last_domain_modes: HashMap::new(),
        transactions: TransactionEngine::new(
            PathBuf::from("/var/lib/optid/recovery-unit"),
            "unit-generation".to_string(),
        ),
        systemd: Box::<FakeSystemd>::default(),
    }
}

include!("tests/production.rs");
include!("tests/systemd.rs");
include!("tests/unit.rs");
include!("tests/s2d.rs");
include!("tests/s3d.rs");

// ── the systemd `[not set]` placeholder (daemon-side copy) ─────────────────

/// The daemon read a property with no value as if `[not set]` were the value,
/// then tried to write that string back. Its log filled with
/// `Failed to parse CPUWeight= value '[not set]': Invalid argument` once per
/// cycle, the cgroup weight lever could never undo itself, and the pending
/// record then blocked the next start. `recovery.rs` was fixed first; this is
/// the daemon's own copy of the same logic.
#[test]
fn the_unset_placeholder_is_not_a_value() {
    assert!(is_unset_placeholder("[not set]"));
    assert!(is_unset_placeholder("  [not set]  "));
    assert!(is_unset_placeholder(""));
    assert!(is_unset_placeholder("   "));
    assert!(!is_unset_placeholder("150"));
    assert!(!is_unset_placeholder("infinity"));
}

/// A bare `CPUWeight=` is what an unset-restore leaves behind. Counting it as an
/// explicit setting is what made a correct restore look like a failed one.
#[test]
fn a_bare_assignment_is_not_an_explicit_setting() {
    assert!(!assigns_a_value("CPUWeight=", "CPUWeight"));
    assert!(!assigns_a_value("  CPUWeight=  ", "CPUWeight"));
    assert!(assigns_a_value("CPUWeight=150", "CPUWeight"));
}

/// A property whose name merely prefixes another must not match.
#[test]
fn a_neighbouring_property_does_not_match() {
    assert!(!assigns_a_value("CPUWeightFoo=150", "CPUWeight"));
    assert!(!assigns_a_value("IOWeight=150", "CPUWeight"));
    assert!(!assigns_a_value("[Slice]", "CPUWeight"));
}
