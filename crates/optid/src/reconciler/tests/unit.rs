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
    assert_eq!(
        report.intentional_v1_only,
        BTreeSet::from(["systemd".to_string()])
    );
}

#[test]
fn f4_restart_hydrates_real_legacy_journal_and_ownership() {
    let state_dir = PathBuf::from("/run/optid-test");
    let device = PathBuf::from("/sys/test/device");
    let original = state_dir.join("original_rpm_abc");
    let applied = state_dir.join("applied_rpm_abc");
    let kernel = MemoryKernel::new();
    kernel.add_dir(Path::new("/run"), &state_dir);
    kernel.add_dir(&state_dir, &original);
    kernel.add_dir(&state_dir, &applied);
    kernel.write_raw(&original, "/sys/test/device\non\n1000");
    kernel.write_raw(&applied, "1\nauto\n2000");
    kernel.write_raw(&device.join("power/control"), "auto");
    kernel.write_raw(&device.join("power/autosuspend_delay_ms"), "2000");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("hydrate");
    let hydrated = reconciler.targets.get("runtime-pm:abc").expect("target");
    assert_eq!(hydrated.ownership, OwnershipState::Optid);
    assert!(hydrated.restore_pending);
}

#[test]
fn f4_malformed_typed_state_fails_closed() {
    let state_dir = PathBuf::from("/run/optid-malformed");
    let state_file = state_dir.join(STATE_FILE);
    let kernel = MemoryKernel::new();
    kernel.add_dir(Path::new("/run"), &state_dir);
    kernel.add_dir(&state_dir, &state_file);
    kernel.write_raw(&state_file, "{not-json");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);

    let error = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .err()
    .expect("malformed state must fail closed");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn f4_systemd_property_lifecycle_distinguishes_unset_and_explicit() {
    let fake = FakeSystemd::default();
    fake.seed("background.slice", "CPUWeight", false, "100");
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
    assert_eq!(restored.value, "100");
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
    reconciler
        .record_restore_outcome(&plan, &outcome, &io)
        .expect("record restore outcome");
    let state = reconciler.targets.get("a").expect("state");
    assert_eq!(state.ownership, OwnershipState::Optid);
    assert!(state.restore_pending);
    assert!(reconciler.plan_restores().is_empty());
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
