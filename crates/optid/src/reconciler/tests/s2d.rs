use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::envelope::GateDisposition;
use crate::kernel_io::{Clock, FaultKernel, KernelRead, KernelWrite};

#[derive(Clone)]
struct S2dSharedKernel(Arc<MemoryKernel>);

impl KernelRead for S2dSharedKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.0.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.0.read_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.0.exists(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.0.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        match self.0.read_link(path) {
            Ok(target) => Ok(target),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.0.canonicalize(path),
            Err(error) => Err(error),
        }
    }
}

impl KernelWrite for S2dSharedKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        self.0.write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.0.write_state_file(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.0.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.0.remove_file(path)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.0.append(path, text)
    }
}

impl Clock for S2dSharedKernel {
    fn now_unix(&self) -> u64 {
        self.0.now_unix()
    }
}

#[derive(Clone)]
struct S2dTraceKernel {
    inner: S2dSharedKernel,
    events: Arc<Mutex<Vec<String>>>,
}

impl S2dTraceKernel {
    fn push(&self, event: String) {
        self.events
            .lock()
            .expect("S2D trace mutex poisoned")
            .push(event);
    }
}

impl KernelRead for S2dTraceKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.inner.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.read_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.canonicalize(path)
    }
}

impl KernelWrite for S2dTraceKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        self.push(format!("kernel:{}", path.display()));
        self.inner.write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.push(format!("state:{}", path.display()));
        self.inner.write_state_file(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.push(format!("mkdir:{}", path.display()));
        self.inner.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.push(format!("rename:{}->{}", from.display(), to.display()));
        self.inner.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.push(format!("remove:{}", path.display()));
        self.inner.remove_file(path)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.push(format!("append:{}", path.display()));
        self.inner.append(path, text)
    }
}

impl Clock for S2dTraceKernel {
    fn now_unix(&self) -> u64 {
        self.inner.now_unix()
    }
}

#[derive(Clone)]
struct S2dMismatchKernel {
    inner: S2dSharedKernel,
    target: PathBuf,
    mismatch_once: Arc<AtomicBool>,
}

impl KernelRead for S2dMismatchKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.inner.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.read_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.canonicalize(path)
    }
}

impl KernelWrite for S2dMismatchKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        if path == self.target
            && value == "10"
            && !self.mismatch_once.swap(true, Ordering::SeqCst)
        {
            self.inner.0.write_raw(path, "11");
            Ok(())
        } else {
            self.inner.write(path, value)
        }
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.inner.write_state_file(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.inner.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.inner.remove_file(path)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.inner.append(path, text)
    }
}

impl Clock for S2dMismatchKernel {
    fn now_unix(&self) -> u64 {
        self.inner.now_unix()
    }
}

fn s2d_armed_actuator(state_dir: PathBuf, kernel: Box<dyn KernelIo>) -> Actuator {
    let mut actuator = Actuator::new_with_kernel(state_dir, kernel);
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

fn s2d_reconciler(
    state_dir: PathBuf,
    recovery_dir: PathBuf,
    actuator: &mut Actuator,
    generation: &str,
) -> Reconciler {
    let mut reconciler = Reconciler::load_with_systemd_and_recovery(
        state_dir,
        recovery_dir.clone(),
        actuator,
        Box::<FakeSystemd>::default(),
    )
    .expect("load S2D reconciler");
    reconciler.transactions = TransactionEngine::new(recovery_dir, generation.to_string());
    reconciler
}

fn s2d_desired(path: &Path, value: &str) -> DesiredTarget {
    DesiredTarget {
        target_id: format!(
            "vm-sysctl:{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .expect("fixture file name")
        ),
        domain: "vm_sysctl".to_string(),
        target: TargetKind::KernelValue {
            path: path.to_path_buf(),
        },
        desired: StoredValue::Scalar {
            value: value.to_string(),
        },
        legacy_journal_key: Some(format!(
            "vm_{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .expect("fixture file name")
        )),
    }
}

fn s2d_assert_journal_denied(outcome: &ActionOutcome) {
    let gate = outcome
        .gates
        .iter()
        .find(|gate| gate.stage == GateStage::RecoveryJournal)
        .expect("S2D outcome must expose recovery-journal gate");
    assert_eq!(gate.disposition, GateDisposition::Denied);
    assert_eq!(gate.reason, GateReasonCode::JournalFailed);
    assert!(outcome.targets.iter().all(|target| !target.write_attempted));
    assert!(outcome
        .targets
        .iter()
        .all(|target| target.pipeline_stage == PipelineStage::Journal));
}

#[test]
fn s2d_durable_record_is_synced_before_production_write() {
    let state_dir = PathBuf::from("/run/optid-s2d-order");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-order");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let events = Arc::new(Mutex::new(Vec::new()));
    let trace = S2dTraceKernel {
        inner: S2dSharedKernel(Arc::clone(&memory)),
        events: Arc::clone(&events),
    };
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(trace));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir.clone(), &mut actuator, "order-generation");
    reconciler.transactions.set_trace(Arc::clone(&events));
    let action = vm_action(&path, "10");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("capture baseline");
    events.lock().expect("trace mutex").clear();
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply through S2D");
    assert!(matches!(
        outcome.targets[0].readback,
        ReadbackOutcome::Confirmed { .. }
    ));

    let events = events.lock().expect("trace mutex");
    let temp_write = events
        .iter()
        .position(|event| event.starts_with("state:") && event.contains("recovery-order"))
        .expect("transaction temp write");
    let file_sync = events
        .iter()
        .position(|event| event.starts_with("sync_file:") && event.contains("recovery-order"))
        .expect("transaction file fsync");
    let publish = events
        .iter()
        .position(|event| event.starts_with("rename:") && event.contains("recovery-order"))
        .expect("transaction publish");
    let directory_sync = events
        .iter()
        .position(|event| event == &format!("sync_dir:{}", recovery_dir.display()))
        .expect("recovery directory fsync");
    let kernel_write = events
        .iter()
        .position(|event| event == &format!("kernel:{}", path.display()))
        .expect("production kernel write");
    assert!(temp_write < file_sync);
    assert!(file_sync < publish);
    assert!(publish < directory_sync);
    assert!(directory_sync < kernel_write);
}

#[test]
fn s2d_full_disk_before_publish_refuses_mutation() {
    let state_dir = PathBuf::from("/run/optid-s2d-full-disk");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-full-disk");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let shared = S2dSharedKernel(Arc::clone(&memory));
    let engine = TransactionEngine::new(recovery_dir.clone(), "full-disk-generation".to_string());
    let action = vm_action(&path, "10");
    let temp = engine.temp_path(&engine.record_path(&action.stable_target_id()));
    let fault = FaultKernel::new(Box::new(shared));
    fault.fail_next_write(temp, io::ErrorKind::StorageFull);
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(fault));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "full-disk-generation");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("capture baseline");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("journal failure is a typed outcome");
    s2d_assert_journal_denied(&outcome);
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
}

#[test]
fn s2d_fsync_failure_refuses_mutation() {
    let state_dir = PathBuf::from("/run/optid-s2d-fsync");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-fsync");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let shared = S2dSharedKernel(Arc::clone(&memory));
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(shared));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "fsync-generation");
    let action = vm_action(&path, "10");
    let temp = reconciler
        .transactions
        .temp_path(&reconciler.transactions.record_path(&action.stable_target_id()));
    reconciler
        .transactions
        .fail_next_sync_file(temp, io::ErrorKind::Other);

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("capture baseline");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("fsync failure is a typed outcome");
    s2d_assert_journal_denied(&outcome);
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
}

#[test]
fn s2d_partial_publish_failure_refuses_mutation() {
    let state_dir = PathBuf::from("/run/optid-s2d-partial-publish");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-partial-publish");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let shared = S2dSharedKernel(Arc::clone(&memory));
    let engine =
        TransactionEngine::new(recovery_dir.clone(), "partial-publish-generation".to_string());
    let action = vm_action(&path, "10");
    let final_path = engine.record_path(&action.stable_target_id());
    let temp_path = engine.temp_path(&final_path);
    let fault = FaultKernel::new(Box::new(shared));
    fault.fail_next_rename(
        temp_path,
        final_path.clone(),
        io::ErrorKind::Interrupted,
    );
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(fault));
    let mut reconciler = s2d_reconciler(
        state_dir,
        recovery_dir,
        &mut actuator,
        "partial-publish-generation",
    );

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("capture baseline");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("publish failure is a typed outcome");
    s2d_assert_journal_denied(&outcome);
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
    assert!(!memory.exists(&final_path));
}

#[test]
fn s2d_prepared_record_covers_crash_before_write() {
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-crash-window");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = MemoryKernel::new();
    memory.write_raw(&path, "60");
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let engine = TransactionEngine::new(recovery_dir, "crash-window-generation".to_string());

    let handle = engine
        .prepare(&memory, &action, &desired, &original)
        .expect("durably prepare");
    let record = engine
        .load_record(&memory, &handle.path)
        .expect("load prepared record");
    assert_eq!(record.phase, TransactionPhase::Prepared);
    assert_eq!(record.original, original);
    assert_eq!(record.intended, desired.desired);
    assert_eq!(memory.read_to_string(&path).expect("no write yet"), "60");
}

#[test]
fn s2d_write_failure_compensates_and_cleans() {
    let state_dir = PathBuf::from("/run/optid-s2d-write-failure");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-write-failure");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let fault = FaultKernel::new(Box::new(S2dSharedKernel(Arc::clone(&memory))));
    fault.fail_next_write(path.clone(), io::ErrorKind::PermissionDenied);
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(fault));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "write-failure-generation");
    let action = vm_action(&path, "10");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("capture baseline");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("failed write is compensated");
    assert!(outcome.targets[0].write_attempted);
    assert_eq!(memory.read_to_string(&path).expect("original restored"), "60");
    assert!(outcome.targets[0]
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("S2D compensation")));
    assert!(reconciler
        .transactions
        .active_records(actuator.kernel.as_ref())
        .expect("list records")
        .is_empty());
}

#[test]
fn s2d_readback_mismatch_compensates_and_cleans() {
    let state_dir = PathBuf::from("/run/optid-s2d-readback");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-readback");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let mismatch = S2dMismatchKernel {
        inner: S2dSharedKernel(Arc::clone(&memory)),
        target: path.clone(),
        mismatch_once: Arc::new(AtomicBool::new(false)),
    };
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(mismatch));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "readback-generation");
    let action = vm_action(&path, "10");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("capture baseline");
    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("mismatch is compensated");
    assert!(matches!(
        outcome.targets[0].readback,
        ReadbackOutcome::Mismatch { .. }
    ));
    assert_eq!(outcome.targets[0].ownership, OwnershipState::Unowned);
    assert_eq!(memory.read_to_string(&path).expect("original restored"), "60");
    assert!(reconciler
        .transactions
        .active_records(actuator.kernel.as_ref())
        .expect("list records")
        .is_empty());
}

#[test]
fn s2d_stale_generation_is_rejected() {
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-stale");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = MemoryKernel::new();
    memory.write_raw(&path, "60");
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    TransactionEngine::new(recovery_dir.clone(), "old-generation".to_string())
        .prepare(&memory, &action, &desired, &original)
        .expect("old generation prepares record");

    let error = TransactionEngine::new(recovery_dir, "new-generation".to_string())
        .prepare(&memory, &action, &desired, &original)
        .expect_err("new generation must not reuse unresolved record");
    assert_eq!(error.kind, TransactionErrorKind::StaleGeneration);
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
}

#[test]
fn s2d_path_reuse_identity_mismatch_is_rejected() {
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-path-reuse");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    let io = S2dSharedKernel(Arc::clone(&memory));
    memory.write_raw(&path, "60");
    memory.write_link(&path, Path::new("/devices/original/swappiness"));
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let engine = TransactionEngine::new(recovery_dir, "identity-generation".to_string());
    engine
        .prepare(&io, &action, &desired, &original)
        .expect("prepare original identity");
    memory.write_link(&path, Path::new("/devices/reused/swappiness"));

    let error = engine
        .prepare(&io, &action, &desired, &original)
        .expect_err("path reuse must be rejected");
    assert_eq!(error.kind, TransactionErrorKind::IdentityMismatch);
    let error = engine
        .validate_handback(&io, &desired.target_id)
        .expect_err("path reuse must also block restoration");
    assert_eq!(error.kind, TransactionErrorKind::IdentityMismatch);
    engine
        .finish_handback(&io, &desired.target_id, true)
        .expect_err("replacement is not a removed target");
    assert!(io.exists(&engine.record_path(&desired.target_id)));
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
}

#[test]
fn s2d_removed_target_relinquishes_and_other_targets_restore() {
    let state_dir = PathBuf::from("/run/optid-s2d-removal");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-removal");
    let device = PathBuf::from("/sys/bus/pci/devices/0000:02:00.0");
    let control = device.join("power/control");
    let delay = device.join("power/autosuspend_delay_ms");
    let vm = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&control, "on");
    memory.write_raw(&delay, "2000");
    memory.write_raw(&vm, "60");
    let mut actuator = s2d_armed_actuator(
        state_dir.clone(),
        Box::new(S2dSharedKernel(Arc::clone(&memory))),
    );
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "removal-generation");
    let actions = [runtime_pm_action(&device, 100), vm_action(&vm, "10")];
    reconciler.prepare_cycle(&actions, &mut actuator).expect("prepare");
    for action in &actions {
        reconciler.apply_action(&mut actuator, action).expect("apply");
    }
    assert_eq!(memory.read_to_string(&control).expect("applied control"), "auto");
    assert_eq!(memory.read_to_string(&vm).expect("applied VM value"), "10");

    let removed = FaultKernel::new(Box::new(S2dSharedKernel(Arc::clone(&memory))));
    removed.hide_path(control.clone()).hide_path(delay.clone());
    actuator.kernel = Box::new(removed);
    let outcomes = reconciler.restore_all_owned(&mut actuator).expect("hand back");
    assert_eq!(outcomes.len(), 2);
    let gone = outcomes.iter().find(|outcome| {
        outcome.reason == OutcomeReasonCode::OwnershipRelinquished
    }).expect("removed device explicitly relinquished");
    assert!(!gone.write_attempted);
    assert_eq!(gone.pending_restore, RestoreState::NotApplicable);
    assert!(outcomes.iter().any(|outcome| outcome.reason == OutcomeReasonCode::RestoreApplied));
    assert_eq!(memory.read_to_string(&vm).expect("surviving target restored"), "60");
    assert_eq!(memory.read_to_string(&control).expect("no write to vanished control"), "auto");
    assert_eq!(memory.read_to_string(&delay).expect("no write to vanished delay"), "100");
    assert!(reconciler.transactions.active_records(actuator.kernel.as_ref())
        .expect("records compacted").is_empty());
    assert!(reconciler.restore_all_owned(&mut actuator).expect("idempotent handback").is_empty());
}

#[test]
fn s2d_missing_runtime_pm_member_keeps_the_whole_undo_record() {
    let state_dir = PathBuf::from("/run/optid-s2d-partial-removal");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-partial-removal");
    let device = PathBuf::from("/sys/bus/pci/devices/0000:02:00.0");
    let control = device.join("power/control");
    let delay = device.join("power/autosuspend_delay_ms");
    let vm = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&vm, "60");
    memory.write_raw(&control, "on");
    memory.write_raw(&delay, "2000");
    let mut actuator = s2d_armed_actuator(
        state_dir.clone(),
        Box::new(S2dSharedKernel(Arc::clone(&memory))),
    );
    let mut reconciler = s2d_reconciler(
        state_dir, recovery_dir, &mut actuator, "partial-removal-generation",
    );
    let actions = [runtime_pm_action(&device, 100), vm_action(&vm, "10")];
    reconciler.prepare_cycle(&actions, &mut actuator).expect("prepare");
    for action in &actions {
        reconciler.apply_action(&mut actuator, action).expect("apply");
    }
    let removed = FaultKernel::new(Box::new(S2dSharedKernel(Arc::clone(&memory))));
    removed.hide_path(delay);
    actuator.kernel = Box::new(removed);
    let (result, messages) = capture_notifications(|| reconciler.restore_all_owned(&mut actuator));
    result.expect_err("a surviving member is not removal");
    assert!(messages.is_empty(), "failed handback must not notify the watchdog");
    let records = reconciler.transactions.active_records(actuator.kernel.as_ref()).expect("undo records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase, TransactionPhase::Committed);
    assert_eq!(memory.read_to_string(&vm).expect("unrelated target restored despite failure"), "60");
    assert_eq!(memory.read_to_string(&control).expect("no unverified write"), "auto");
}

#[test]
fn s2d_removed_target_cannot_relinquish_another_generation() {
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-removed-stale");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let shared = S2dSharedKernel(Arc::clone(&memory));
    let desired = s2d_desired(&path, "10");
    let old = TransactionEngine::new(recovery_dir.clone(), "old-generation".to_string());
    let handle = old.prepare(&shared, &vm_action(&path, "10"), &desired,
        &StoredValue::Scalar { value: "60".to_string() }).expect("prepare old record");
    let removed = FaultKernel::new(Box::new(shared));
    removed.hide_path(path);
    let new = TransactionEngine::new(recovery_dir, "new-generation".to_string());
    let error = new.validate_handback(&removed, &desired.target_id).expect_err("stale owner");
    assert_eq!(error.kind, TransactionErrorKind::StaleGeneration);
    new.finish_handback(&removed, &desired.target_id, true).expect_err("retain stale record");
    assert!(removed.exists(&handle.path));
}

#[test]
fn s2d_removed_target_sync_failure_preserves_the_undo_record() {
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let shared = S2dSharedKernel(Arc::clone(&memory));
    let desired = s2d_desired(&path, "10");
    let engine = TransactionEngine::new(
        PathBuf::from("/var/lib/optid/recovery-removed-sync"), "sync-generation".to_string(),
    );
    let handle = engine.prepare(&shared, &vm_action(&path, "10"), &desired,
        &StoredValue::Scalar { value: "60".to_string() }).expect("prepare record");
    let removed = FaultKernel::new(Box::new(shared));
    removed.hide_path(path);
    engine.fail_next_sync_file(engine.temp_path(&handle.path), io::ErrorKind::StorageFull);
    engine.finish_handback(&removed, &desired.target_id, true).expect_err("failed durable relinquishment");
    let record = engine.load_record(&removed, &handle.path).expect("undo record retained");
    assert_eq!(record.phase, TransactionPhase::Prepared);
    // MemoryKernel keeps directory listings separate from file writes.
    memory.add_dir_entry(&engine.root, &handle.path);
    memory.add_dir_entry(&engine.root, &engine.temp_path(&handle.path));
    assert!(verify_journal_health(&engine, &removed).is_err());
}

#[test]
fn s2d_repeated_compensation_is_idempotent() {
    let state_dir = PathBuf::from("/run/optid-s2d-repeat");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-repeat");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let mut actuator = s2d_armed_actuator(
        state_dir.clone(),
        Box::new(S2dSharedKernel(Arc::clone(&memory))),
    );
    let reconciler =
        s2d_reconciler(state_dir, recovery_dir.clone(), &mut actuator, "repeat-generation");
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let handle = reconciler
        .transactions
        .prepare(actuator.kernel.as_ref(), &action, &desired, &original)
        .expect("prepare transaction");
    actuator.kernel.write(&path, "10").expect("simulate landed write");

    assert_eq!(
        reconciler
            .compensation_for_handle(&mut actuator, &handle)
            .expect("first compensation"),
        CompensationDisposition::Restored
    );
    assert_eq!(
        reconciler
            .compensation_for_handle(&mut actuator, &handle)
            .expect("repeated compensation"),
        CompensationDisposition::AlreadyRestored
    );
    assert_eq!(memory.read_to_string(&path).expect("original restored"), "60");
}

#[test]
fn s2d_external_drift_relinquishes_without_overwrite() {
    let state_dir = PathBuf::from("/run/optid-s2d-drift");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-drift");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let shared = S2dSharedKernel(Arc::clone(&memory));
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(shared.clone()));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "drift-generation");
    let action = vm_action(&path, "10");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare apply");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("commit apply transaction");
    shared.write(&path, "25").expect("external owner writes");
    reconciler.prepare_cycle(&[], &mut actuator).expect("drop desired");
    let outcomes = reconciler.reconcile(&mut actuator).expect("reconcile drift");

    assert_eq!(outcomes[0].reason, OutcomeReasonCode::OwnershipRelinquished);
    assert!(!outcomes[0].write_attempted);
    assert_eq!(memory.read_to_string(&path).expect("external value remains"), "25");
    assert!(reconciler
        .transactions
        .active_records(actuator.kernel.as_ref())
        .expect("list records")
        .is_empty());
}

#[test]
fn s2d_production_commit_then_verified_restore_compacts() {
    let state_dir = PathBuf::from("/run/optid-s2d-cleanup");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-cleanup");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&path, "60");
    let mut actuator = s2d_armed_actuator(
        state_dir.clone(),
        Box::new(S2dSharedKernel(Arc::clone(&memory))),
    );
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "cleanup-generation");
    let action = vm_action(&path, "10");

    reconciler
        .prepare_cycle(std::slice::from_ref(&action), &mut actuator)
        .expect("prepare apply");
    reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply");
    let committed = reconciler
        .transactions
        .active_records(actuator.kernel.as_ref())
        .expect("list committed records");
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].phase, TransactionPhase::Committed);

    reconciler.prepare_cycle(&[], &mut actuator).expect("drop desired");
    let outcomes = reconciler.reconcile(&mut actuator).expect("restore");
    assert_eq!(outcomes[0].reason, OutcomeReasonCode::RestoreApplied);
    assert_eq!(memory.read_to_string(&path).expect("restored"), "60");
    assert!(reconciler
        .transactions
        .active_records(actuator.kernel.as_ref())
        .expect("records compacted")
        .is_empty());
}


#[test]
fn s2d_record_path_is_stable_across_generations() {
    let root = PathBuf::from("/var/lib/optid/recovery-stable-name");
    let target = "vm-sysctl:swappiness";
    let first = TransactionEngine::new(root.clone(), "generation-one".to_string());
    let second = TransactionEngine::new(root, "generation-two".to_string());

    assert_eq!(first.record_path(target), second.record_path(target));
}

#[test]
fn s2d_stale_generation_handback_does_not_compact() {
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-stale-handback");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = MemoryKernel::new();
    memory.write_raw(&path, "60");
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let old = TransactionEngine::new(recovery_dir.clone(), "old-generation".to_string());
    let handle = old
        .prepare(&memory, &action, &desired, &original)
        .expect("old generation prepares record");

    let current = TransactionEngine::new(recovery_dir, "new-generation".to_string());
    let error = current
        .finish_handback(&memory, &action.stable_target_id(), true)
        .expect_err("new generation must not compact stale recovery evidence");
    assert_eq!(error.kind, TransactionErrorKind::StaleGeneration);
    assert!(memory.exists(&handle.path));
}

#[test]
fn s2d_compensation_attempts_every_target_after_one_failure() {
    let state_dir = PathBuf::from("/run/optid-s2d-all-compensation");
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-all-compensation");
    let stale_path = PathBuf::from("/proc/sys/vm/dirty_bytes");
    let current_path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&stale_path, "1000");
    memory.write_raw(&current_path, "60");
    let mut actuator = s2d_armed_actuator(
        state_dir.clone(),
        Box::new(S2dSharedKernel(Arc::clone(&memory))),
    );
    let reconciler = s2d_reconciler(
        state_dir,
        recovery_dir.clone(),
        &mut actuator,
        "current-generation",
    );

    let stale_action = vm_action(&stale_path, "2000");
    let stale_desired = s2d_desired(&stale_path, "2000");
    let stale_original = StoredValue::Scalar {
        value: "1000".to_string(),
    };
    let stale_handle = TransactionEngine::new(recovery_dir, "old-generation".to_string())
        .prepare(
            actuator.kernel.as_ref(),
            &stale_action,
            &stale_desired,
            &stale_original,
        )
        .expect("prepare stale target");
    let stale_record_path = stale_handle.path.clone();

    let current_action = vm_action(&current_path, "10");
    let current_desired = s2d_desired(&current_path, "10");
    let current_original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let current_handle = reconciler
        .transactions
        .prepare(
            actuator.kernel.as_ref(),
            &current_action,
            &current_desired,
            &current_original,
        )
        .expect("prepare current target");
    let current_record_path = current_handle.path.clone();

    actuator
        .kernel
        .write(&stale_path, "2000")
        .expect("simulate stale landed write");
    actuator
        .kernel
        .write(&current_path, "10")
        .expect("simulate current landed write");

    let mut handles = std::collections::BTreeMap::new();
    handles.insert(stale_action.stable_target_id(), stale_handle);
    handles.insert(current_action.stable_target_id(), current_handle);
    let error = reconciler
        .compensate_all(&mut actuator, &handles)
        .expect_err("first stale target still reports an error");
    assert_eq!(error.kind, TransactionErrorKind::StaleGeneration);
    assert_eq!(
        memory.read_to_string(&stale_path).expect("stale value retained"),
        "2000"
    );
    assert_eq!(
        memory
            .read_to_string(&current_path)
            .expect("later target restored"),
        "60"
    );
    assert!(memory.exists(&stale_record_path));
    assert!(!memory.exists(&current_record_path));
}

#[test]
fn s2d_production_daemon_run_uses_persistent_transaction_protocol() {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::args::{Args, ForegroundMode};
    use crate::kernel_io::with_real_kernel_override;
    use crate::shim::conflict::with_conflict_checker_override;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock after Unix epoch")
        .as_nanos();
    let state_dir = std::env::temp_dir().join(format!(
        "optid_s2d_production_surface_{}_{}",
        std::process::id(),
        nonce
    ));
    fs::create_dir_all(&state_dir).expect("create daemon state directory");
    let config_path = state_dir.join("policy.toml");
    let default_policy_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/optid/policy.toml");
    let mut policy =
        fs::read_to_string(default_policy_path).expect("read default policy fixture");
    policy = policy.replace(
        "high_swappiness_requires_zram = true",
        "high_swappiness_requires_zram = false",
    );
    policy.push_str(
        r#"
[domains.cpu_epp]
mode = "off"
[domains.platform_profile]
mode = "off"
[domains.cgroup_reweight]
mode = "off"
[domains.vm_sysctl]
mode = "actuate"
[domains.cpu_dma_latency]
mode = "off"
[domains.device_resume_latency]
mode = "off"
[domains.runtime_pm]
mode = "off"
[domains.pci_aspm]
mode = "off"
[domains.sata_alpm]
mode = "off"
[domains.backlight]
mode = "off"
"#,
    );
    fs::write(&config_path, policy).expect("write S2D policy fixture");

    let swappiness = PathBuf::from("/proc/sys/vm/swappiness");
    let dirty_background = PathBuf::from("/proc/sys/vm/dirty_background_bytes");
    let dirty = PathBuf::from("/proc/sys/vm/dirty_bytes");
    let memory = Arc::new(MemoryKernel::new());
    memory.write_raw(&swappiness, "60");
    memory.write_raw(&dirty_background, "0");
    memory.write_raw(&dirty, "0");
    memory.advance_clock(1_700_000_100);
    let events = Arc::new(Mutex::new(Vec::new()));
    let trace = S2dTraceKernel {
        inner: S2dSharedKernel(Arc::clone(&memory)),
        events: Arc::clone(&events),
    };
    let args = Args {
        apply: true,
        once: true,
        help: false,
        version: false,
        interval_sec: 1,
        state_dir: state_dir.clone(),
        config_path,
        allowlist: false,
        foreground: ForegroundMode::Off,
    };

    // This exercises the real production `--apply` path end to end, which
    // must not be at the mercy of whichever policy daemon happens to be
    // active on the host running the suite (see
    // `shim::conflict::with_conflict_checker_override`) — `tuned` being
    // active on the dev/CI host previously downgraded `--apply` to dry-run
    // here and hid every S2D assertion below it.
    with_conflict_checker_override(
        |_service| false,
        || with_real_kernel_override(Box::new(trace), || crate::run(args)),
    )
    .expect("production daemon run must complete through S2D");

    assert_eq!(
        memory.read_to_string(&swappiness).expect("restored swappiness"),
        "60"
    );
    assert_eq!(
        memory
            .read_to_string(&dirty_background)
            .expect("restored dirty background"),
        "0"
    );
    assert_eq!(memory.read_to_string(&dirty).expect("restored dirty"), "0");
    let events = events.lock().expect("trace mutex");
    assert!(events.iter().any(|event| {
        event.starts_with("state:") && event.contains("s2d-recovery")
    }));
    assert!(events.iter().any(|event| {
        event.starts_with("rename:") && event.contains("s2d-recovery")
    }));
    assert!(events
        .iter()
        .any(|event| event == &format!("kernel:{}", swappiness.display())));
    assert!(events.iter().any(|event| {
        event.starts_with("remove:") && event.contains("s2d-recovery")
    }));

    fs::remove_dir_all(&state_dir).expect("remove daemon state directory");
}
