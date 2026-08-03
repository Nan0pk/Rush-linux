#!/usr/bin/env python3
from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one replacement, found {count}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


replace_once(
    "crates/optid/src/reconciler/transaction.rs",
    '''    fn finish_handback(
        &self,
        io: &dyn KernelIo,
        target_id: &str,
        relinquished: bool,
    ) -> Result<(), TransactionError> {''',
    '''    fn validate_handback(
        &self,
        io: &dyn KernelIo,
        target_id: &str,
    ) -> Result<(), TransactionError> {
        let path = self.record_path(target_id);
        if !io.exists(&path) {
            return Ok(());
        }
        let record = self.load_record(io, &path)?;
        self.validate_generation_and_identity(io, &record)
    }

    fn finish_handback(
        &self,
        io: &dyn KernelIo,
        target_id: &str,
        relinquished: bool,
    ) -> Result<(), TransactionError> {''',
)

replace_once(
    "crates/optid/src/reconciler/apply.rs",
    '''            } else {
                actuator.execute_restore(&plan, self.systemd.as_ref())?
            };''',
    '''            } else {
                self.transactions
                    .validate_handback(actuator.kernel.as_ref(), &plan.target_id)
                    .map_err(io::Error::from)?;
                actuator.execute_restore(&plan, self.systemd.as_ref())?
            };''',
)

replace_once(
    "crates/optid/src/reconciler/tests/production.rs",
    '''    let outcomes = restarted.reconcile(&mut actuator).expect("restore");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].reason, OutcomeReasonCode::RestoreApplied);
    assert_eq!(
        actuator.kernel.read_to_string(&path).expect("restored"),
        "60"
    );''',
    '''    let error = restarted
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
    );''',
)

replace_once(
    "docs/plans/optid-package-status.toml",
    '''  "crates/optid/src/reconciler/tests.rs",
  "crates/optid/src/reconciler/tests/s2d.rs",''',
    '''  "crates/optid/src/reconciler/tests.rs",
  "crates/optid/src/reconciler/tests/production.rs",
  "crates/optid/src/reconciler/tests/s2d.rs",''',
)
replace_once(
    "docs/plans/optid-package-status.toml",
    '''all_targets_compensated = "s2d_compensation_attempts_every_target_after_one_failure"''',
    '''all_targets_compensated = "s2d_compensation_attempts_every_target_after_one_failure"
restart_handoff_to_s3d = "f4_restart_hydrates_typed_vm_state_and_restores"''',
)
replace_once(
    "docs/architecture/optid-s2d-persistent-transactions.md",
    '''- stale-generation handback preserving recovery evidence;
- compensation continuing across every target after an earlier failure; and''',
    '''- stale-generation handback preserving recovery evidence;
- process restart refusing handback before mutation until S3D recovers the
  previous generation;
- compensation continuing across every target after an earlier failure; and''',
)
