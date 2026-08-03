#[test]
fn f4_production_runtime_pm_transition_restores_baseline() {
    let state_dir = PathBuf::from("/run/optid-f4-runtime");
    let device = PathBuf::from("/sys/bus/usb/devices/1-1");
    let control = device.join("power/control");
    let delay = device.join("power/autosuspend_delay_ms");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&control, "on");
    kernel.write_raw(&delay, "1000");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load reconciler");
    let action = runtime_pm_action(&device, 2000);
    let _ = reconciler.detect_transitions(
        Some(false),
        WorkloadClass::Idle,
        Mode::Battery,
        &HashMap::from([(Domain::RuntimePm, DomainMode::Actuate)]),
    );

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare battery-idle cycle");
    let applied = reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply runtime PM");
    assert!(applied.targets.iter().all(|target| {
        matches!(target.readback, ReadbackOutcome::Confirmed { .. })
            && target.ownership == OwnershipState::Optid
    }));
    assert_eq!(
        actuator.kernel.read_to_string(&control).expect("control"),
        "auto"
    );
    assert_eq!(
        actuator.kernel.read_to_string(&delay).expect("delay"),
        "2000"
    );

    let transitions = reconciler.detect_transitions(
        Some(true),
        WorkloadClass::Interactive,
        Mode::Balanced,
        &HashMap::from([(Domain::RuntimePm, DomainMode::Actuate)]),
    );
    assert!(!transitions.is_empty());
    let stale = reconciler
        .prepare_cycle(&[], &mut actuator)
        .expect("prepare AC/interactive cycle");
    assert_eq!(stale.len(), 1);
    let restored = reconciler.reconcile(&mut actuator).expect("restore baseline");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].reason, OutcomeReasonCode::RestoreApplied);
    assert_eq!(restored[0].ownership, OwnershipState::Unowned);
    assert!(matches!(
        restored[0].readback,
        ReadbackOutcome::Confirmed { .. }
    ));
    assert_eq!(
        actuator.kernel.read_to_string(&control).expect("control"),
        "on"
    );
    assert_eq!(
        actuator.kernel.read_to_string(&delay).expect("delay"),
        "1000"
    );
}

#[test]
fn f4_production_external_drift_relinquishes_without_overwrite() {
    let state_dir = PathBuf::from("/run/optid-f4-drift");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&path, "60");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load reconciler");
    let action = vm_action(&path, "10");
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply");
    actuator.kernel.write(&path, "25").expect("external drift");
    reconciler.prepare_cycle(&[], &mut actuator).expect("stale");

    let outcomes = reconciler.reconcile(&mut actuator).expect("reconcile");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].reason,
        OutcomeReasonCode::OwnershipRelinquished
    );
    assert!(!outcomes[0].write_attempted);
    assert_eq!(
        actuator.kernel.read_to_string(&path).expect("read drift"),
        "25"
    );
}

#[test]
fn f4_production_coalesces_confirmed_value_and_writes_one_change() {
    let state_dir = PathBuf::from("/run/optid-f4-coalesce");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&path, "60");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load reconciler");
    let first = vm_action(&path, "10");
    reconciler
        .prepare_cycle(std::slice::from_ref(&first), &mut actuator)
        .expect("prepare first");
    reconciler
        .apply_action(&mut actuator, &first)
        .expect("apply first");

    reconciler
        .prepare_cycle(std::slice::from_ref(&first), &mut actuator)
        .expect("prepare same");
    let same = reconciler
        .apply_action(&mut actuator, &first)
        .expect("coalesced apply");
    assert_eq!(same.targets[0].write_outcome, WriteOutcome::Redundant);
    assert!(!same.targets[0].write_attempted);

    let changed = vm_action(&path, "20");
    reconciler
        .prepare_cycle(std::slice::from_ref(&changed), &mut actuator)
        .expect("prepare changed");
    let changed_outcome = reconciler
        .apply_action(&mut actuator, &changed)
        .expect("apply changed");
    assert!(changed_outcome.targets[0].write_attempted);
    assert_eq!(
        actuator.kernel.read_to_string(&path).expect("changed value"),
        "20"
    );
}

#[test]
fn f4_active_kernel_drift_relinquishes_and_stays_handed_back() {
    let state_dir = PathBuf::from("/run/optid-f4-active-drift");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&path, "60");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load reconciler");
    let action = vm_action(&path, "10");
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare first");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply first");

    actuator.kernel.write(&path, "25").expect("external drift");
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
    assert_eq!(
        actuator.kernel.read_to_string(&path).expect("drift remains"),
        "25"
    );

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
    assert_eq!(
        actuator.kernel.read_to_string(&path).expect("still external"),
        "25"
    );
}

#[test]
fn f4_production_multiple_targets_restore_only_disappeared_target() {
    let state_dir = PathBuf::from("/run/optid-f4-multiple");
    let swappiness = PathBuf::from("/proc/sys/vm/swappiness");
    let dirty = PathBuf::from("/proc/sys/vm/dirty_bytes");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&swappiness, "60");
    kernel.write_raw(&dirty, "0");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load reconciler");
    let first = vm_action(&swappiness, "10");
    let second = vm_action(&dirty, "1048576");
    reconciler
        .prepare_cycle(&[first.clone(), second.clone()], &mut actuator)
        .expect("prepare both");
    for action in [&first, &second] {
        reconciler
            .apply_action(&mut actuator, action)
            .expect("apply target");
    }

    reconciler
        .prepare_cycle(std::slice::from_ref(&second), &mut actuator)
        .expect("keep one target");
    let outcomes = reconciler.reconcile(&mut actuator).expect("restore one");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].target_id, first.stable_target_id());
    assert_eq!(
        actuator
            .kernel
            .read_to_string(&swappiness)
            .expect("swappiness"),
        "60"
    );
    assert_eq!(
        actuator.kernel.read_to_string(&dirty).expect("dirty"),
        "1048576"
    );
}

#[test]
fn f4_restart_hydrates_typed_vm_state_and_restores() {
    let state_dir = PathBuf::from("/run/optid-f4-restart-typed");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&path, "60");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let action = vm_action(&path, "10");
    {
        let mut reconciler = Reconciler::load_with_systemd(
            state_dir.clone(),
            &mut actuator,
            Box::<FakeSystemd>::default(),
        )
        .expect("load reconciler");
        reconciler
            .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
            .expect("prepare");
        reconciler
            .apply_action(&mut actuator, &action)
            .expect("apply");
    }
    let mut restarted = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("restart hydrate");
    restarted
        .prepare_cycle(&[], &mut actuator)
        .expect("new desired state omits target");
    let error = restarted
        .reconcile(&mut actuator)
        .expect_err("S3D must recover the previous generation before handback");
    let detail = error.to_string();
    assert!(detail.contains("StaleGeneration"), "{detail}");
    assert_eq!(
        actuator
            .kernel
            .read_to_string(&path)
            .expect("previous generation value remains untouched"),
        "10"
    );
}

#[test]
fn f4_production_device_disappearance_relinquishes_ownership() {
    let state_dir = PathBuf::from("/run/optid-f4-device-gone");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let kernel = MemoryKernel::new();
    kernel.write_raw(&path, "60");
    let mut actuator = armed_actuator(state_dir.clone(), kernel);
    let mut reconciler = Reconciler::load_with_systemd(
        state_dir,
        &mut actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load reconciler");
    let action = vm_action(&path, "10");
    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply");
    actuator.kernel.remove_file(&path).expect("remove target");
    reconciler.prepare_cycle(&[], &mut actuator).expect("stale");
    let outcomes = reconciler.reconcile(&mut actuator).expect("reconcile");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].reason,
        OutcomeReasonCode::OwnershipRelinquished
    );
    assert!(!outcomes[0].write_attempted);
}
