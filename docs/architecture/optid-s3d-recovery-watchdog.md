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

## Candidate proof

GitHub Actions run `30853374668` used separate builder and verifier jobs. The
builder created exact source commit
`3339e63df6721cd3a15989f4ab1364644ddae81e` and exported it as immutable
artifact `8871514800` with digest
`sha256:04a3a584b5c576fbc2464bed9734903d28d88f7d73d355a019d2e8d1786f20c8`.

A fresh Ubuntu 24.04 verifier checked out that exact commit and passed:

- workspace format, compile, and warning-free clippy;
- every impacted F4, S2D, and S3D acceptance/regression test individually;
- the complete workspace suite, including 440 passing optid daemon tests;
- recovery CLI and systemd ordering tests;
- current-work, package-ledger, generated README, repository-policy, and
  finish-work gates.

The retained integrated-verification log is artifact `8871568706`, digest
`sha256:6a7f22e90d5549cd5e8870635416b13543b2141c04408c6baeba977a8eee361c`.
The verifier refreshed the F4 and S2D receipts because S3D touches their shared
reconciliation proof paths. S3D remains a builder `candidate`; this evidence is
not a post-merge completion receipt.

## Boundaries

S3D does not pre-open hardware descriptors or install Landlock; that is S4D.
It does not persist domain/HWID circuit breakers or controlled canary re-entry;
that is S5D. It does not implement new topology discovery. The apply unit is
prepared for systemd-managed cold restart, while actual hotplug-triggered
topology rebuilding remains coupled to the later sealed-capability lifecycle.
