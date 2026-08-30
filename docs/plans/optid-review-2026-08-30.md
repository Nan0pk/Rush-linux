# Optid foundation and safety review — 2026-08-30

The maintainer approved continuing after merging PR #439. Two independent,
read-only reviewers checked merged source
`fabd3646d91b90a3c0ce1dbb6d851299d519b89e`. The next task is to repair the
remaining runtime gaps before fresh package completion claims.

## Executed proof

[Merged-main CI](https://github.com/Nan0pk/Rush-linux/actions/runs/33301673921),
Linux job `99230923797`, tested that exact merge. Both reviewers inspected
the log independently. The workspace passed 707 tests, including 543 daemon
tests. The feature-enabled simulation ran six model tests, three simulation
CLI tests, and six enabled-versus-off evidence tests. Formatting, Clippy and
all-target/all-feature compilation passed.

All 87 mapped acceptance tests for the four foundations and four safety
packages appeared as passed in the containing suites. They were not rerun
individually with `--exact`. The reviewers did not execute Rust locally.
This is useful regression evidence, not proof that unmapped requirements
are implemented or that hardware is safe or more efficient.

## Findings and repair order

The following are source-review findings unless executed proof is stated.
Source locations refer to the merge above.

1. **Preserve partial restoration and truthful health signals.** Runtime-PM
   restoration writes control before delay. A delay failure leaves a partly
   restored pair; the next attempt compares it with the old applied pair,
   calls it external drift, and discards the undo record. A typed
   `RestorationFailed` outcome also reaches watchdog notification because it
   is inside an `Ok` result. Retry exhaustion incorrectly labels unresolved
   ownership as relinquished. Repair in `reconciler/restore.rs` and
   `reconciler/apply.rs`: retain the exact progress confirmed after our own
   writes, retain the baseline and durable undo, bound retries, and withhold
   health notifications while restoration remains unresolved. Keep structured
   failures available for diagnostics and circuit handling. Do not accept
   arbitrary mixtures of baseline and intended values as owned. Regression
   tests enter the production reconciler through an armed actuator.
2. **Validate independent recovery's authority and device identity.**
   `recovery.rs` accepts an arbitrary JSON `operation` without checking its
   correspondence to the target. Its independent writes do not use the typed
   capability path contract. Both it and `reconciler/transaction.rs` identify
   targets by canonical pathname alone, so an unresolved record cannot
   distinguish replacement hardware at the same pathname. Reuse the existing
   capability vocabulary, constrain operations and targets, and define durable
   hardware identity before permitting those records to write. Required
   negative tests include unknown operations, operation/path mismatch and
   same-path replacement. A relinquished record already avoids target access
   after PR #439; that does not protect unresolved records.
3. **Connect restore failures and crashes to persistent quarantine.**
   `CircuitScope::for_restore` creates an operation and hardware identity that
   do not match `CircuitScope::from_action`; exact scope matching therefore
   misses these failures on later actuation. Ordinary admission also has no
   durable marker for an unfinished run. A kill or hang before outcome
   recording can therefore escape the failure count. Require a test that
   records a real restore failure then evaluates the actual action, plus
   process-kill/restart evidence that persistent quarantine prevents reapply.
4. **Implement or explicitly reject emergency stabilization.** Recovery records
   a stabilization description but never executes a typed stabilizer after
   failed rollback. Retain evidence and avoid calling this restored. Any
   implementation must follow the accepted per-lever contract, not invent
   hardware defaults.
5. **Report why writes are disarmed correctly.** The failed policy-reload
   branch in `main.rs` passes `false` to `ActionOutcome::suppressed`, producing
   `ApplyNotRequested` even when started with `--apply`. The last good policy
   remains protected, but the per-action diagnosis is wrong. Extend the
   existing production reload simulation to verify the reason.
6. **Finish the injectable waiting boundary.** Kernel I/O and observation time
   are injected, but daemon waiting still uses real `Instant` and
   `thread::sleep`; `RealKernel::wait` bypasses the override. The real event
   reactor is a later package. That does not prove the foundation's promised
   injectable waiting seam. Add deterministic daemon-wait coverage before
   claiming that boundary complete.

## Boundaries

No package is recertified by this review. Capability/domain configuration has
support for its mapped contract, but blanket foundation/safety certification
is not justified. The current repair remains within complete desired-state
reconciliation; other findings remain open. No runtime defaults, allowlist,
hardware claims or release milestones change.

Later live proof must build production `optid` and `optid-recover` with default
features. `test-simulation` replaces transaction fsync with existence checks,
so the old shadow script's all-feature build cannot prove production durability.
The existing capability-sealing workflow proves its test service, not the
production recovery/watchdog lifecycle. Reuse its kernel checks after repairs,
and separately demonstrate production recovery ordering, watchdog/SIGKILL
handling, failed recovery blocking restart, and durable crash quarantine.

Independent reviewers: `verify_foundations` and `verify_safety`, separate from
the implementation worker. Both independently confirmed the partial-restore
defect; the safety reviewer also identified the watchdog and retry-path gaps.
