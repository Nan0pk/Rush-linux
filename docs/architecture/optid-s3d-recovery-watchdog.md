# S3D Independent Recovery and Watchdog Supervision

**Status:** Builder candidate for package S3D
**Architecture:** D2 fail-passive capability sealing

## Scope

S3D adds two bounded mechanisms without adding a permanent broker or a
steady-state IPC hop:

1. `optid-recover`, a one-shot executable that consumes S2D recovery records
   before automatic actuation starts or restarts; and
2. systemd notification from the synchronous reconciler path only after a
   complete transaction/readback/reconciliation cycle and a healthy journal.

The recovery executable contains no policy parser, classifier, D-Bus server,
session bridge, or async runtime.

## Recovery ordering

`optid-apply.service` runs `optid-recover` as `ExecStartPre` on every initial
start and every supervisor-managed restart. A failed recovery exits with status
78, prevents automatic actuation, and is excluded from the normal restart loop.
The separate `optid-recover.service` provides an explicit boot/manual one-shot
unit and is ordered before the apply service.

Recovery is idempotent:

- a prepared record whose original is still present is recorded as already
  restored and compacted;
- an intended or transaction-partial value is rolled back to the exact captured
  original and verified;
- external drift is relinquished without overwrite;
- canonical identity mismatch, malformed evidence, write failure, or readback
  mismatch leaves the transaction record in place and fails closed; and
- every outcome is appended durably before a resolved record is removed.

## Watchdog semantics

The watchdog message is emitted synchronously from `Reconciler::reconcile`
after apply/readback/compensation, transition handback, state persistence, and
journal validation all complete. The first healthy cycle sends `READY=1` and
`WATCHDOG=1`; later healthy cycles send only `WATCHDOG=1`.

An unpublished temp record, malformed record, stale generation, identity
mismatch, non-committed residual phase, or notification failure prevents the
heartbeat and returns an error to the daemon. There is no independent heartbeat
thread that could falsely report health while the control path is stuck.

## Boundaries

S3D does not pre-open hardware descriptors or install Landlock; that is S4D.
It does not persist domain/HWID circuit breakers or controlled canary re-entry;
that is S5D. It does not implement new topology discovery. The apply unit is
prepared for systemd-managed cold restart, while actual hotplug-triggered
topology rebuilding remains coupled to the later sealed-capability lifecycle.
