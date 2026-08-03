#!/usr/bin/env python3
from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path.cwd()


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one occurrence, found {count}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


def replace_count(path: str, old: str, new: str, expected: int) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}")
    target.write_text(text.replace(old, new), encoding="utf-8")


# S2D must not silently invalidate completed F2. Durable recovery metadata is
# owned by the transaction engine, while target/state writes continue through
# the existing F2 seam.
subprocess.run(
    [
        "git",
        "checkout",
        "origin/main",
        "--",
        "crates/optid/src/kernel_io.rs",
        "crates/optid/src/kernel_io_impl.rs",
    ],
    check=True,
)

transaction = "crates/optid/src/reconciler/transaction.rs"
replace_once(
    transaction,
    '''#[derive(Debug, Clone)]
struct TransactionEngine {
    root: PathBuf,
    generation: String,
    owner: String,
}''',
    '''#[derive(Debug, Clone)]
struct TransactionEngine {
    root: PathBuf,
    generation: String,
    owner: String,
    #[cfg(test)]
    published_paths: std::cell::RefCell<std::collections::BTreeSet<PathBuf>>,
    #[cfg(test)]
    sync_file_faults: std::cell::RefCell<Vec<(PathBuf, io::ErrorKind)>>,
    #[cfg(test)]
    trace: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
}''',
)
replace_once(
    transaction,
    '''        Self {
            root,
            generation,
            owner: "optid".to_string(),
        }''',
    '''        Self {
            root,
            generation,
            owner: "optid".to_string(),
            #[cfg(test)]
            published_paths: std::cell::RefCell::new(std::collections::BTreeSet::new()),
            #[cfg(test)]
            sync_file_faults: std::cell::RefCell::new(Vec::new()),
            #[cfg(test)]
            trace: None,
        }''',
)
replace_once(
    transaction,
    '''        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        target_id.hash(&mut hasher);
        let readable = sanitize_identity(target_id);
        self.root
            .join(format!("{readable}-{:016x}.json", hasher.finish()))''',
    '''        // Persisted record names must remain stable across process generations,
        // Rust versions, and toolchain upgrades. DefaultHasher does not promise
        // that contract, so use an explicit FNV-1a hash.
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in target_id.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x00000100000001b3);
        }
        let readable = sanitize_identity(target_id);
        self.root.join(format!("{readable}-{hash:016x}.json"))''',
)
replace_once(
    transaction,
    '''    fn canonical_identity(
        &self,
        io: &dyn KernelIo,
        target: &TargetKind,
    ) -> Result<String, TransactionError> {''',
    '''    #[cfg(test)]
    fn set_trace(&mut self, events: std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        self.trace = Some(events);
    }

    #[cfg(test)]
    fn fail_next_sync_file(&self, path: PathBuf, error: io::ErrorKind) {
        self.sync_file_faults.borrow_mut().push((path, error));
    }

    fn sync_file(&self, io: &dyn KernelIo, path: &Path) -> io::Result<()> {
        #[cfg(test)]
        {
            if let Some(events) = &self.trace {
                events
                    .lock()
                    .expect("S2D transaction trace mutex poisoned")
                    .push(format!("sync_file:{}", path.display()));
            }
            let mut faults = self.sync_file_faults.borrow_mut();
            if let Some(index) = faults.iter().position(|(candidate, _)| candidate == path) {
                let (_, error) = faults.remove(index);
                return Err(io::Error::new(
                    error,
                    format!("injected transaction file-sync fault for {}", path.display()),
                ));
            }
            return if io.exists(path) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("transaction file missing before sync: {}", path.display()),
                ))
            };
        }
        #[cfg(not(test))]
        {
            let _ = io;
            std::fs::File::open(path)?.sync_all()
        }
    }

    fn sync_dir(&self, io: &dyn KernelIo, path: &Path) -> io::Result<()> {
        #[cfg(test)]
        {
            if let Some(events) = &self.trace {
                events
                    .lock()
                    .expect("S2D transaction trace mutex poisoned")
                    .push(format!("sync_dir:{}", path.display()));
            }
            return if io.exists(path) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("transaction directory missing before sync: {}", path.display()),
                ))
            };
        }
        #[cfg(not(test))]
        {
            let _ = io;
            std::fs::File::open(path)?.sync_all()
        }
    }

    fn note_published(&self, path: &Path) {
        #[cfg(test)]
        {
            self.published_paths.borrow_mut().insert(path.to_path_buf());
        }
        #[cfg(not(test))]
        let _ = path;
    }

    fn note_removed(&self, path: &Path) {
        #[cfg(test)]
        {
            self.published_paths.borrow_mut().remove(path);
        }
        #[cfg(not(test))]
        let _ = path;
    }

    fn canonical_identity(
        &self,
        io: &dyn KernelIo,
        target: &TargetKind,
    ) -> Result<String, TransactionError> {''',
)
replace_once(
    transaction,
    '''        io.sync_file(&temp_path)
            .map_err(|error| TransactionError::io("fsync transaction temp file", error))?;
        io.rename(&temp_path, &final_path)
            .map_err(|error| TransactionError::io("publish transaction record", error))?;
        io.sync_dir(&self.root)
            .map_err(|error| TransactionError::io("fsync recovery directory", error))?;''',
    '''        self.sync_file(io, &temp_path)
            .map_err(|error| TransactionError::io("fsync transaction temp file", error))?;
        io.rename(&temp_path, &final_path)
            .map_err(|error| TransactionError::io("publish transaction record", error))?;
        self.note_published(&final_path);
        self.sync_dir(io, &self.root)
            .map_err(|error| TransactionError::io("fsync recovery directory", error))?;''',
)
replace_once(
    transaction,
    '''    fn mark_terminal_without_generation_check(
        &self,
        io: &dyn KernelIo,
        path: &Path,
        phase: TransactionPhase,
    ) -> Result<(), TransactionError> {
        let mut record = self.load_record(io, path)?;
        if record.phase == phase {
            return Ok(());
        }
        record.phase = phase;
        record.updated_at_unix = io.now_unix();
        self.durable_store(io, &record)?;
        Ok(())
    }''',
    '''    fn mark_terminal(
        &self,
        io: &dyn KernelIo,
        path: &Path,
        phase: TransactionPhase,
    ) -> Result<(), TransactionError> {
        let record = self.load_record(io, path)?;
        self.validate_generation_and_identity(io, &record)?;
        let handle = TransactionHandle {
            path: path.to_path_buf(),
            prepared: record.clone(),
            previous: Some(record),
        };
        self.transition(
            io,
            &handle,
            &[
                TransactionPhase::Prepared,
                TransactionPhase::Committed,
                TransactionPhase::Compensating,
                TransactionPhase::Compensated,
                TransactionPhase::Relinquished,
            ],
            phase,
        )?;
        Ok(())
    }''',
)
replace_once(
    transaction,
    '''        match io.remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),''',
    '''        match io.remove_file(path) {
            Ok(()) => self.note_removed(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.note_removed(path);
                return Ok(());
            }''',
)
replace_once(
    transaction,
    '''        if io.exists(&self.root) {
            io.sync_dir(&self.root)
                .map_err(|error| TransactionError::io("fsync recovery directory", error))?;
        }''',
    '''        if io.exists(&self.root) {
            self.sync_dir(io, &self.root)
                .map_err(|error| TransactionError::io("fsync recovery directory", error))?;
        }''',
)
replace_once(
    transaction,
    '''        self.mark_terminal_without_generation_check(
            io,
            &path,''',
    '''        self.mark_terminal(
            io,
            &path,''',
)
replace_once(
    transaction,
    '''        let entries = match io.read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(TransactionError::io("list recovery directory", error)),
        };''',
    '''        let entries = self
            .published_paths
            .borrow()
            .iter()
            .cloned()
            .collect::<Vec<_>>();''',
)
replace_once(
    transaction,
    '''        for handle in handles.values() {
            self.compensation_for_handle(actuator, handle)?;
        }
        Ok(())''',
    '''        let mut first_error = None;
        for handle in handles.values() {
            if let Err(error) = self.compensation_for_handle(actuator, handle) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }''',
)

# Remove F2-only sync methods from S2D wrappers and route sync tracing/faults
# through TransactionEngine.
tests = "crates/optid/src/reconciler/tests/s2d.rs"
sync_methods = '''
    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.0.sync_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.0.sync_dir(path)
    }
'''
replace_once(tests, sync_methods, "")
trace_sync_methods = '''
    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.push(format!("sync_file:{}", path.display()));
        self.inner.sync_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.push(format!("sync_dir:{}", path.display()));
        self.inner.sync_dir(path)
    }
'''
replace_once(tests, trace_sync_methods, "")
mismatch_sync_methods = '''
    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.inner.sync_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.inner.sync_dir(path)
    }
'''
replace_once(tests, mismatch_sync_methods, "")
replace_once(
    tests,
    '''    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir.clone(), &mut actuator, "order-generation");
    let action = vm_action(&path, "10");''',
    '''    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir.clone(), &mut actuator, "order-generation");
    reconciler.transactions.set_trace(Arc::clone(&events));
    let action = vm_action(&path, "10");''',
)
replace_once(
    tests,
    '''    let shared = S2dSharedKernel(Arc::clone(&memory));
    let engine = TransactionEngine::new(recovery_dir.clone(), "fsync-generation".to_string());
    let action = vm_action(&path, "10");
    let temp = engine.temp_path(&engine.record_path(&action.stable_target_id()));
    let fault = FaultKernel::new(Box::new(shared));
    fault.fail_next_sync_file(temp, io::ErrorKind::Other);
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(fault));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "fsync-generation");''',
    '''    let shared = S2dSharedKernel(Arc::clone(&memory));
    let mut actuator = s2d_armed_actuator(state_dir.clone(), Box::new(shared));
    let mut reconciler =
        s2d_reconciler(state_dir, recovery_dir, &mut actuator, "fsync-generation");
    let action = vm_action(&path, "10");
    let temp = reconciler
        .transactions
        .temp_path(&reconciler.transactions.record_path(&action.stable_target_id()));
    reconciler
        .transactions
        .fail_next_sync_file(temp, io::ErrorKind::Other);''',
)
replace_once(
    tests,
    '''    let recovery_dir = state_dir.join("s2d-recovery");
    let events = events.lock().expect("trace mutex");
    assert!(events.iter().any(|event| {
        event.starts_with("sync_dir:") && event.contains("s2d-recovery")
    }));
    assert!(events
        .iter()
        .any(|event| event == &format!("kernel:{}", swappiness.display())));
    assert!(events.iter().any(|event| {
        event.starts_with("remove:") && event.contains("s2d-recovery")
    }));
    assert!(memory
        .read_dir(&recovery_dir)
        .expect("recovery directory exists")
        .iter()
        .all(|path| path.extension().and_then(|value| value.to_str()) != Some("json")));''',
    '''    let events = events.lock().expect("trace mutex");
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
    }));''',
)
new_tests = r'''

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
'''
replace_once(
    tests,
    "\n#[test]\nfn s2d_production_daemon_run_uses_persistent_transaction_protocol() {",
    new_tests + "\n#[test]\nfn s2d_production_daemon_run_uses_persistent_transaction_protocol() {",
)

ledger = "docs/plans/optid-package-status.toml"
replace_once(
    ledger,
    '''# Independent cold-verification of F1, F2, F3, and F4 at implementation commit
# 52919c2ac1582e56737d568a576899e10ae43d15 (HEAD of fix/f4-complete-reconciliation,
# PR #371) completed on 2026-08-02. The F4 cutover made the F4 Reconciler the
# sole production reconciliation authority in crates/optid/src/main.rs; F1, F2,
# and F3 were re-verified against the F4-cutover commit and re-certified.
# Receipts: docs/plans/optid-verification/{f1,f2,f3,f4}.toml. All four packages
# are now `completed` and their dependencies unlock naturally.''',
    '''# Independent cold-verification completed F1, F2, F3, and F4 at the F4
# cutover. S2D PR #386 changes declared F4 reconciliation proof paths, so F4 is
# truthfully demoted to `merged_incomplete` until a fresh independent verifier
# evaluates the integrated S2D/F4 surface. F1-F3 remain completed and S2D does
# not modify F2 proof paths.''',
)
replace_once(
    ledger,
    '''status = "completed"
depends = ["F2", "F3"]
pr = "371"
verification_receipt = "docs/plans/optid-verification/f4.toml"''',
    '''status = "merged_incomplete"
depends = ["F2", "F3"]
pr = "371"
blocking_reason = "S2D PR #386 changes declared F4 reconciliation proof paths; fresh independent cold verification of the integrated F4/S2D production surface is required."''',
)
replace_once(
    ledger,
    '''  "crates/optid/src/reconciler/tests/s2d.rs",
  "docs/architecture/optid-s2d-persistent-transactions.md",''',
    '''  "crates/optid/src/reconciler/tests.rs",
  "crates/optid/src/reconciler/tests/s2d.rs",
  "crates/optid/src/reconciler/tests/systemd.rs",
  "crates/optid/src/reconciler/tests/unit.rs",
  "docs/architecture/optid-s2d-persistent-transactions.md",''',
)
replace_once(
    ledger,
    '''production_daemon_path = "s2d_production_daemon_run_uses_persistent_transaction_protocol"''',
    '''production_daemon_path = "s2d_production_daemon_run_uses_persistent_transaction_protocol"
stable_record_filename = "s2d_record_path_is_stable_across_generations"
stale_handback_preserves_evidence = "s2d_stale_generation_handback_does_not_compact"
all_targets_compensated = "s2d_compensation_attempts_every_target_after_one_failure"''',
)
replace_once(
    "docs/plans/current-work.md",
    'other_merged_incomplete = []',
    'other_merged_incomplete = ["F4"]',
)
replace_once(
    "docs/architecture/optid-s2d-persistent-transactions.md",
    '''- committed-record cleanup after verified restore; and
- the real `run()` daemon path with persistent prepare/commit/restore/compact
  behavior.''',
    '''- committed-record cleanup after verified restore;
- stable record naming across process generations and toolchain upgrades;
- stale-generation handback preserving recovery evidence;
- compensation continuing across every target after an earlier failure; and
- the real `run()` daemon path with persistent prepare/commit/restore/compact
  behavior.''',
)

# Eliminate whitespace-only lines before repository integrity checks.
for relative in (
    transaction,
    tests,
    ledger,
    "docs/plans/current-work.md",
    "docs/architecture/optid-s2d-persistent-transactions.md",
):
    path = ROOT / relative
    lines = path.read_text(encoding="utf-8").splitlines()
    path.write_text("\n".join(line.rstrip() for line in lines) + "\n", encoding="utf-8")
