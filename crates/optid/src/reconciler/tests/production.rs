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

struct RuntimePmRestoreFixture {
    reconciler: Reconciler,
    actuator: Actuator,
    memory: Arc<MemoryKernel>,
    control: PathBuf,
    delay: PathBuf,
}

#[derive(Clone)]
struct PostWriteReadFailureKernel {
    inner: S2dSharedKernel,
    control: PathBuf,
    fail_read: Arc<std::sync::atomic::AtomicBool>,
}

impl KernelRead for PostWriteReadFailureKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if path == self.control
            && self
                .fail_read
                .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "post-write readback fault"));
        }
        self.inner.read_to_string(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> { self.inner.read_dir(path) }
    fn exists(&self, path: &Path) -> bool { self.inner.exists(path) }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> { self.inner.read_link(path) }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> { self.inner.canonicalize(path) }
}

impl KernelWrite for PostWriteReadFailureKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        let result = self.inner.write(path, value);
        if path == self.control && value == "on" && result.is_ok() {
            self.fail_read.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        result
    }
    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> { self.inner.write_state_file(path, value) }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> { self.inner.create_dir_all(path) }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> { self.inner.rename(from, to) }
    fn remove_file(&self, path: &Path) -> io::Result<()> { self.inner.remove_file(path) }
    fn append(&self, path: &Path, text: &str) -> io::Result<()> { self.inner.append(path, text) }
}

impl Clock for PostWriteReadFailureKernel {
    fn now_unix(&self) -> u64 { self.inner.now_unix() }
}

impl RuntimePmRestoreFixture {
    fn new(name: &str) -> Self {
        let state_dir = PathBuf::from(format!("/run/optid-f4-{name}"));
        let recovery_dir = PathBuf::from(format!("/var/lib/optid/recovery-f4-{name}"));
        let device = PathBuf::from("/sys/bus/usb/devices/1-1");
        let control = device.join("power/control");
        let delay = device.join("power/autosuspend_delay_ms");
        let memory = Arc::new(MemoryKernel::new());
        memory.write_raw(&control, "on");
        memory.write_raw(&delay, "1000");
        let mut actuator = s2d_armed_actuator(
            state_dir.clone(),
            Box::new(S2dSharedKernel(Arc::clone(&memory))),
        );
        let mut reconciler = s2d_reconciler(
            state_dir, recovery_dir, &mut actuator, "partial-restore-generation",
        );
        let action = runtime_pm_action(&device, 2000);
        reconciler.prepare_cycle(std::slice::from_ref(&action), &mut actuator).unwrap();
        reconciler.apply_action(&mut actuator, &action).unwrap();
        reconciler.prepare_cycle(&[], &mut actuator).unwrap();
        Self { reconciler, actuator, memory, control, delay }
    }

    fn fail_write_once(&mut self, path: PathBuf) {
        let fault = FaultKernel::new(Box::new(S2dSharedKernel(Arc::clone(&self.memory))));
        fault.fail_next_write(path, io::ErrorKind::PermissionDenied);
        self.actuator.kernel = Box::new(fault);
    }

    fn records(&self) -> Vec<TransactionRecord> {
        self.reconciler.transactions.active_records(self.actuator.kernel.as_ref()).unwrap()
    }
}

#[test]
fn f4_partial_runtime_pm_restore_retries_confirmed_progress() {
    let mut fixture = RuntimePmRestoreFixture::new("partial-retry");
    fixture.fail_write_once(fixture.delay.clone());
    let first = fixture.reconciler.reconcile(&mut fixture.actuator).unwrap();
    assert_eq!(first[0].reason, OutcomeReasonCode::RestoreFailed);
    assert_eq!(fixture.memory.read_to_string(&fixture.control).unwrap(), "on");
    assert_eq!(fixture.memory.read_to_string(&fixture.delay).unwrap(), "2000");
    assert_eq!(fixture.records().len(), 1, "partial restore must keep its undo record");
    let saved: PersistedState = serde_json::from_str(
        &fixture.memory.read_to_string(&fixture.reconciler.state_dir.join(STATE_FILE)).unwrap(),
    ).unwrap();
    let saved = saved.targets.values().next().unwrap();
    assert_eq!(saved.baseline, Some(StoredValue::RuntimePm {
        control: "on".to_string(), delay: Some("1000".to_string()),
    }));
    assert_eq!(saved.last_confirmed, Some(StoredValue::RuntimePm {
        control: "on".to_string(), delay: Some("2000".to_string()),
    }));

    let (second, messages) = capture_notifications(|| {
        fixture.reconciler.reconcile(&mut fixture.actuator)
    });
    let second = second.unwrap();
    assert_eq!(second[0].reason, OutcomeReasonCode::RestoreApplied);
    assert_eq!(fixture.memory.read_to_string(&fixture.control).unwrap(), "on");
    assert_eq!(fixture.memory.read_to_string(&fixture.delay).unwrap(), "1000");
    assert!(fixture.records().is_empty(), "verified whole-target restore may compact");
    assert!(messages.iter().any(|message| message.contains("WATCHDOG=1")));
}

#[test]
fn f4_runtime_pm_unwritten_baseline_mixture_is_external_drift() {
    let mut fixture = RuntimePmRestoreFixture::new("unwritten-mixture");
    fixture.memory.write_raw(&fixture.control, "on");
    let outcomes = fixture.reconciler.reconcile(&mut fixture.actuator).unwrap();
    assert_eq!(outcomes[0].reason, OutcomeReasonCode::OwnershipRelinquished);
    assert!(!outcomes[0].write_attempted);
    assert_eq!(fixture.memory.read_to_string(&fixture.delay).unwrap(), "2000");
}

#[test]
fn f4_unconfirmed_partial_restore_keeps_undo_record() {
    let mut fixture = RuntimePmRestoreFixture::new("unconfirmed");
    let kernel = PostWriteReadFailureKernel {
        inner: S2dSharedKernel(Arc::clone(&fixture.memory)),
        control: fixture.control.clone(),
        fail_read: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    fixture.actuator.kernel = Box::new(kernel.clone());
    // The fault must not be armed here. `execute_restore` reads the target
    // before it writes it, and `fail_read` is a one-shot: pre-arming spends it
    // on that pre-write read, so the restore aborts before writing and the
    // post-write readback this test exists to exercise never happens. The
    // kernel's own `write` arms it after the control write lands.
    let first = fixture.reconciler.reconcile(&mut fixture.actuator).unwrap();
    assert_eq!(first[0].reason, OutcomeReasonCode::RestoreFailed);
    assert_eq!(fixture.memory.read_to_string(&fixture.control).unwrap(), "on");
    assert_eq!(fixture.memory.read_to_string(&fixture.delay).unwrap(), "2000");

    let second = fixture.reconciler.reconcile(&mut fixture.actuator).unwrap();
    assert_eq!(second[0].reason, OutcomeReasonCode::RestoreFailed);
    assert!(!second[0].write_attempted);
    assert_eq!(fixture.records().len(), 1, "unconfirmed mutation must retain undo");
}

#[test]
fn f4_partial_runtime_pm_restore_does_not_overwrite_external_change() {
    let mut fixture = RuntimePmRestoreFixture::new("partial-drift");
    fixture.fail_write_once(fixture.delay.clone());
    let first = fixture.reconciler.reconcile(&mut fixture.actuator).unwrap();
    assert_eq!(first[0].reason, OutcomeReasonCode::RestoreFailed);
    fixture.memory.write_raw(&fixture.delay, "3333");

    let second = fixture.reconciler.reconcile(&mut fixture.actuator).unwrap();
    assert_eq!(second[0].reason, OutcomeReasonCode::RestoreFailed);
    assert!(!second[0].write_attempted);
    assert_eq!(fixture.memory.read_to_string(&fixture.delay).unwrap(), "3333");
    assert_eq!(fixture.records().len(), 1, "uncertain progress must keep undo");
}

#[test]
fn f4_restore_failure_withholds_watchdog_through_retry_exhaustion() {
    let mut fixture = RuntimePmRestoreFixture::new("failed-watchdog");
    for _ in 0..MAX_RESTORE_RETRIES {
        fixture.fail_write_once(fixture.control.clone());
        let (result, messages) = capture_notifications(|| {
            fixture.reconciler.reconcile(&mut fixture.actuator)
        });
        let outcomes = result.expect("typed restore failure must remain visible to the caller");
        assert_eq!(outcomes[0].reason, OutcomeReasonCode::RestoreFailed);
        assert!(messages.is_empty(), "failed restore must not report health");
        assert_eq!(fixture.records().len(), 1);
    }
    let target = fixture.reconciler.targets.values().next().unwrap();
    assert_eq!(target.ownership, OwnershipState::Optid);
    assert!(target.restore_pending, "exhaustion is unresolved, not relinquishment");
    let (result, messages) = capture_notifications(|| {
        fixture.reconciler.reconcile(&mut fixture.actuator)
    });
    let outcomes = result.unwrap();
    assert_eq!(outcomes[0].reason, OutcomeReasonCode::RestoreFailed);
    assert!(!outcomes[0].write_attempted, "retry count must remain bounded");
    assert!(messages.is_empty(), "exhausted retries must not resume heartbeats");
    assert_eq!(fixture.records().len(), 1);

    let device = fixture
        .control
        .parent()
        .and_then(Path::parent)
        .expect("runtime-PM device path");
    let action = runtime_pm_action(device, 2000);
    fixture.reconciler.prepare_cycle(std::slice::from_ref(&action), &mut fixture.actuator).unwrap();
    let reapply = fixture.reconciler.apply_action(&mut fixture.actuator, &action).unwrap();
    assert!(reapply.targets.iter().all(|target| !target.write_attempted));
    assert_eq!(fixture.reconciler.targets.values().next().unwrap().retries, MAX_RESTORE_RETRIES);
}
