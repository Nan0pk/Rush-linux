use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::kernel_io::MemoryKernel;

#[derive(Clone, Default)]
struct FakeSystemd {
    properties: Rc<RefCell<BTreeMap<(String, String), SystemdPropertyState>>>,
    fail_property: Rc<RefCell<Option<String>>>,
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
        let previous = self
            .properties
            .borrow()
            .get(&(unit.to_string(), property.to_string()))
            .cloned()
            .unwrap_or(SystemdPropertyState {
                explicit: false,
                value: "default".to_string(),
            });
        self.properties.borrow_mut().insert(
            (unit.to_string(), property.to_string()),
            SystemdPropertyState {
                explicit: value.is_some(),
                value: value.unwrap_or(&previous.value).to_string(),
            },
        );
        Ok(())
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
        systemd: Box::<FakeSystemd>::default(),
    }
}

#[test]
fn f4_multiple_targets_in_one_domain_remain_independent() {
    let targets = BTreeMap::from([
        ("a".to_string(), state("a", None)),
        ("b".to_string(), state("b", Some("auto"))),
    ]);
    let reconciler = reconciler_with(targets, ReconcileMode::V1);
    let plans = reconciler.plan_restores();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].target_id, "a");
}

#[test]
fn f4_missing_baseline_or_confirmation_never_plans_a_pretend_restore() {
    let mut missing_baseline = state("a", None);
    missing_baseline.baseline = None;
    let mut missing_confirmed = state("b", None);
    missing_confirmed.last_confirmed = None;
    let reconciler = reconciler_with(
        BTreeMap::from([
            ("a".to_string(), missing_baseline),
            ("b".to_string(), missing_confirmed),
        ]),
        ReconcileMode::V1,
    );
    assert!(reconciler.plan_restores().is_empty());
}

#[test]
fn f4_shadow_parity_reports_systemd_as_intentional_v1_only() {
    let mut systemd = state("systemd", None);
    systemd.legacy_journal_key = None;
    systemd.target = TargetKind::SystemdProperty {
        unit: "background.slice".to_string(),
        property: "CPUWeight".to_string(),
    };
    let reconciler = reconciler_with(
        BTreeMap::from([
            ("a".to_string(), state("a", None)),
            ("systemd".to_string(), systemd),
        ]),
        ReconcileMode::Shadow,
    );
    let legacy = BTreeSet::from(["rpm_a".to_string()]);
    let report = reconciler.parity_report(&legacy);
    assert!(report.parity);
    assert_eq!(report.intentional_v1_only, BTreeSet::from(["systemd".to_string()]));
}

#[test]
fn f4_restart_hydrates_real_legacy_journal_and_ownership() {
    let io = MemoryKernel::new();
    let state_dir = PathBuf::from("/run/optid-test");
    let device = PathBuf::from("/sys/test/device");
    io.add_dir(Path::new("/run"), &state_dir);
    io.write_raw(
        &state_dir.join("original_rpm_abc"),
        "/sys/test/device\non\n1000",
    );
    io.write_raw(&state_dir.join("applied_rpm_abc"), "1\nauto\n2000");
    io.write_raw(&device.join("power/control"), "auto");
    io.write_raw(&device.join("power/autosuspend_delay_ms"), "2000");
    let reconciler = Reconciler::load_with_systemd(
        state_dir,
        &io,
        Box::<FakeSystemd>::default(),
    )
    .expect("hydrate");
    let hydrated = reconciler.targets.get("runtime-pm:abc").expect("target");
    assert_eq!(hydrated.ownership, OwnershipState::Optid);
    assert!(hydrated.restore_pending);
}

#[test]
fn f4_systemd_property_lifecycle_distinguishes_unset_and_explicit() {
    let fake = FakeSystemd::default();
    fake.properties.borrow_mut().insert(
        ("background.slice".to_string(), "CPUWeight".to_string()),
        SystemdPropertyState {
            explicit: false,
            value: "100".to_string(),
        },
    );
    fake.set_property("background.slice", "CPUWeight", Some("50"))
        .expect("apply");
    let applied = fake
        .read_property("background.slice", "CPUWeight")
        .expect("readback");
    assert!(applied.explicit);
    assert_eq!(applied.value, "50");
    fake.set_property("background.slice", "CPUWeight", None)
        .expect("clear runtime property");
    let restored = fake
        .read_property("background.slice", "CPUWeight")
        .expect("restored readback");
    assert!(!restored.explicit);
}

#[test]
fn f4_retry_policy_is_bounded() {
    let mut target = state("a", None);
    target.retries = MAX_RESTORE_RETRIES - 1;
    let mut reconciler = reconciler_with(
        BTreeMap::from([("a".to_string(), target)]),
        ReconcileMode::V1,
    );
    let plan = reconciler.plan_restores().remove(0);
    let outcome = failed_restore(
        "a",
        true,
        &io::Error::new(io::ErrorKind::PermissionDenied, "blocked"),
    );
    let io = MemoryKernel::new();
    reconciler.record_restore_outcome(&plan, &outcome, &io);
    let state = reconciler.targets.get("a").expect("state");
    assert_eq!(state.ownership, OwnershipState::Relinquished);
    assert!(!state.restore_pending);
}

#[test]
fn f4_transition_detection_covers_ac_workload_mode_and_domain_off() {
    let mut reconciler = reconciler_with(BTreeMap::new(), ReconcileMode::V1);
    reconciler.last_ac = Some(false);
    reconciler.last_workload = WorkloadClass::Idle;
    reconciler.last_mode = Mode::Battery;
    reconciler
        .last_domain_modes
        .insert(Domain::RuntimePm, DomainMode::Actuate);
    let transitions = reconciler.detect_transitions(
        Some(true),
        WorkloadClass::Interactive,
        Mode::Balanced,
        &HashMap::from([(Domain::RuntimePm, DomainMode::Off)]),
    );
    assert!(transitions
        .iter()
        .any(|transition| matches!(transition, Transition::AcChanged { .. })));
    assert!(transitions
        .iter()
        .any(|transition| matches!(transition, Transition::WorkloadChanged { .. })));
    assert!(transitions
        .iter()
        .any(|transition| matches!(transition, Transition::ModeChanged { .. })));
    assert!(transitions
        .iter()
        .any(|transition| matches!(transition, Transition::DomainDisabled { .. })));
}
