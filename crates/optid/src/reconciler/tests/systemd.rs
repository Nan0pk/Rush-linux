#[test]
fn f4_systemd_multiple_properties_apply_restore_and_unset() {
    let state_dir = PathBuf::from("/run/optid-f4-systemd");
    let kernel = MemoryKernel::new();
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let fake = FakeSystemd::default();
    fake.seed("background.slice", "CPUWeight", true, "100");
    fake.seed("background.slice", "IOWeight", false, "100");
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::new(fake.clone()),
    )
    .expect("load reconciler");
    let action = systemd_action(&["CPUWeight=50", "IOWeight=25"]);
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare properties");
    let applied = reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply properties");
    assert_eq!(applied.targets.len(), 2);
    assert!(applied.targets.iter().all(|target| {
        target.responsible_subsystem == ResponsibleSubsystem::Systemd
            && matches!(target.readback, ReadbackOutcome::Confirmed { .. })
    }));
    assert_eq!(fake.state("background.slice", "CPUWeight").value, "50");
    assert_eq!(fake.state("background.slice", "IOWeight").value, "25");

    reconciler.prepare_cycle(&[], &mut actuator).expect("stale");
    let restored = reconciler.reconcile(&mut actuator).expect("restore");
    assert_eq!(restored.len(), 2);
    assert!(restored
        .iter()
        .all(|outcome| outcome.reason == OutcomeReasonCode::RestoreApplied));
    let cpu = fake.state("background.slice", "CPUWeight");
    let io = fake.state("background.slice", "IOWeight");
    assert!(cpu.explicit);
    assert_eq!(cpu.value, "100");
    assert!(!io.explicit);
    assert_eq!(io.value, "100");
}

#[test]
fn f4_systemd_partial_property_failure_is_typed_and_independent() {
    let state_dir = PathBuf::from("/run/optid-f4-systemd-partial");
    let kernel = MemoryKernel::new();
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let fake = FakeSystemd::default();
    fake.seed("background.slice", "CPUWeight", true, "100");
    fake.seed("background.slice", "IOWeight", true, "100");
    *fake.fail_property.borrow_mut() = Some("IOWeight".to_string());
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::new(fake.clone()),
    )
    .expect("load reconciler");
    let action = systemd_action(&["CPUWeight=50", "IOWeight=25"]);
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("partial apply must not abort action");
    assert_eq!(outcome.targets.len(), 2);
    assert!(outcome.targets.iter().any(|target| {
        target.target_id.ends_with("CPUWeight")
            && matches!(target.readback, ReadbackOutcome::Confirmed { .. })
    }));
    assert!(outcome.targets.iter().any(|target| {
        target.target_id.ends_with("IOWeight")
            && matches!(target.write_outcome, WriteOutcome::Failed { .. })
            && target.responsible_subsystem == ResponsibleSubsystem::Systemd
    }));
    assert_eq!(fake.state("background.slice", "CPUWeight").value, "50");
    assert_eq!(fake.state("background.slice", "IOWeight").value, "100");
}

#[test]
fn f4_systemd_readback_mismatch_never_claims_ownership() {
    let state_dir = PathBuf::from("/run/optid-f4-systemd-mismatch");
    let kernel = MemoryKernel::new();
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let fake = FakeSystemd::default();
    fake.seed("background.slice", "CPUWeight", true, "100");
    *fake.mismatch_property.borrow_mut() = Some("CPUWeight".to_string());
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::new(fake),
    )
    .expect("load reconciler");
    let action = systemd_action(&["CPUWeight=50"]);
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply");
    assert!(matches!(
        outcome.targets[0].readback,
        ReadbackOutcome::Mismatch { .. }
    ));
    reconciler.prepare_cycle(&[], &mut actuator).expect("stale");
    assert!(reconciler.plan_restores().is_empty());
}

#[test]
fn f4_systemd_external_drift_is_not_overwritten() {
    let state_dir = PathBuf::from("/run/optid-f4-systemd-drift");
    let kernel = MemoryKernel::new();
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let fake = FakeSystemd::default();
    fake.seed("background.slice", "CPUWeight", true, "100");
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::new(fake.clone()),
    )
    .expect("load reconciler");
    let action = systemd_action(&["CPUWeight=50"]);
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply");
    fake.properties.borrow_mut().insert(
        ("background.slice".to_string(), "CPUWeight".to_string()),
        SystemdPropertyState {
            explicit: true,
            value: "75".to_string(),
        },
    );
    reconciler.prepare_cycle(&[], &mut actuator).expect("stale");
    let outcomes = reconciler.reconcile(&mut actuator).expect("reconcile");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].reason,
        OutcomeReasonCode::OwnershipRelinquished
    );
    assert_eq!(fake.state("background.slice", "CPUWeight").value, "75");
}

#[test]
fn f4_active_systemd_drift_relinquishes_and_stays_handed_back() {
    let state_dir = PathBuf::from("/run/optid-f4-systemd-active-drift");
    let kernel = MemoryKernel::new();
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let fake = FakeSystemd::default();
    fake.seed("background.slice", "CPUWeight", true, "100");
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::new(fake.clone()),
    )
    .expect("load reconciler");
    let action = systemd_action(&["CPUWeight=50"]);
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare first");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply first");

    fake.properties.borrow_mut().insert(
        ("background.slice".to_string(), "CPUWeight".to_string()),
        SystemdPropertyState {
            explicit: true,
            value: "75".to_string(),
        },
    );
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare still desired");
    let drift = reconciler
        .apply_action(&mut actuator, &action)
        .expect("detect active drift");
    assert_eq!(
        drift.targets[0].reason,
        OutcomeReasonCode::OwnershipRelinquished
    );
    assert!(!drift.targets[0].write_attempted);
    assert!(matches!(
        drift.targets[0].readback,
        ReadbackOutcome::Mismatch { .. }
    ));
    assert_eq!(fake.state("background.slice", "CPUWeight").value, "75");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare handed-back cycle");
    let handed_back = reconciler
        .apply_action(&mut actuator, &action)
        .expect("preserve handback");
    assert_eq!(
        handed_back.targets[0].write_outcome,
        WriteOutcome::OwnershipRelinquished
    );
    assert!(!handed_back.targets[0].write_attempted);
    assert_eq!(fake.state("background.slice", "CPUWeight").value, "75");
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
