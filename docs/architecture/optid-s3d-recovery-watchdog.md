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

`optid-apply.service` requires and orders itself after the separate
`optid-recover.service`. The recovery unit is `PartOf=optid-apply.service`, so
every supervisor-managed apply restart starts a fresh one-shot recovery process
before a new daemon process may run. If recovery fails, the required unit start
fails, automatic actuation does not begin, and the recovery unit itself has no
restart loop.

This dependency model is deliberate. `RestartPreventExitStatus=` applies only
to a service's main process and therefore cannot safely suppress retries caused
by a failing `ExecStartPre=` control process. Recovery is consequently not an
`ExecStartPre=` command of the apply service.

The recovery unit remains directly startable at boot or by an operator, and is
ordered before the apply service.

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

### Supervised systemd lifecycle proof

A subsequent audit found that the initial unit used
`RestartPreventExitStatus=78` with `ExecStartPre=optid-recover`. The former
covers only the main service process, so that combination did not prove that a
failed recovery could not enter the apply service's restart policy. The units
were corrected to the explicit `Requires`/`After`/`PartOf` model documented
above, with `Restart=no` on the recovery unit and no recovery `ExecStartPre` in
the apply unit. Both packaged and mkosi-mirrored units carry the same model.

Run `30854862566` then passed format, compile, warning-free clippy, the corrected
static unit tests, and the complete workspace suite. Its hosted-runner attempt
to start the fully sandboxed hardware units stopped before either proof process
because runner-specific hardware paths were absent; it made no claim about the
dependency semantics.

Run `30855133309`, job `91824257421`, separately bound minimal executable units
to the exact dependency directives asserted by the repository tests and proved
the lifecycle on a live Ubuntu 24.04 systemd manager:

- the first daemon process failed;
- systemd scheduled one automatic restart;
- the one-shot recovery unit ran a second time before the replacement daemon
  process started;
- forced recovery exit status 78 failed the required dependency job;
- the daemon process did not execute; and
- the apply unit remained inactive with `NRestarts=0`.

The retained supervised log is artifact `8872165760`, digest
`sha256:23492f24e5c7e22b60f424bf5d25c0fdffaec5d42d1644d2dbe6bb95441bc5ae`.
Its temporary proof workflow removed itself after passing.

## Boundaries

S3D does not pre-open hardware descriptors or install Landlock; that is S4D.
It does not persist domain/HWID circuit breakers or controlled canary re-entry;
that is S5D. It does not implement new topology discovery. The apply unit is
prepared for systemd-managed cold restart, while actual hotplug-triggered
topology rebuilding remains coupled to the later sealed-capability lifecycle.
