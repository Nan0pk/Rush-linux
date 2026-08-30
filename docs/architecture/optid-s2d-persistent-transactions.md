# S2D Persistent Verified Transaction Protocol

**Status:** Builder candidate for package S2D
**Architecture:** D2 fail-passive capability sealing
**Storage root:** `/var/lib/optid/recovery/`

## Purpose

S2D replaces independent runtime-state journal fragments with one versioned,
identity-safe write-ahead transaction protocol consumed by the production F4
reconciler. A mutation either has a durable undo record before the kernel or
systemd write begins, or the mutation is refused.

S2D does not add a helper daemon, IPC, capability sealing, boot recovery, a
watchdog, or circuit breakers. Those remain S3D–S5D work.

## Production integration

The F4 reconciler remains the sole desired-state and restoration authority.
Before `Actuator::apply` or the property-level systemd write path is entered,
the reconciler expands the action into typed targets and prepares one durable
record per target. The same injected `KernelIo` boundary used by production and
fault tests performs journal I/O and target I/O.

Production uses `/var/lib/optid/recovery/`. The shipped apply unit already has
`StateDirectory=optid`, so this path is persistent and writable without adding
a broader `ReadWritePaths` grant. Unit tests redirect the root beneath their
isolated state directory.

## Record schema

Each JSON record is schema-versioned and contains:

- process generation and owner;
- domain and closed `TransactionOperation` variant;
- stable target ID;
- canonical hardware or interface identity;
- typed target descriptor;
- captured original and intended values;
- exact rollback and separately named stabilization methods;
- optional legacy-journal key during migration;
- phase and creation/update timestamps.

The closed operation vocabulary covers CPU EPP, platform profile, systemd
properties, VM sysctls, CPU and device PM QoS, runtime PM, PCIe ASPM, SATA
ALPM, and backlight writes.

## Durability sequence

For every prepare or phase transition:

1. create the recovery directory;
2. serialize to a generation-specific temporary file;
3. write the complete temporary file;
4. `fsync` the temporary file;
5. atomically rename it to the target record;
6. `fsync` the recovery directory.

Only after step 6 succeeds may the production mutation run. A short write,
full disk, failed file sync, failed rename, or failed directory sync denies the
journal gate and produces no target write.

## Lifecycle

`prepared`
: Durable original and intent exist; no write is authorized before this state.

`committed`
: The write completed and typed readback exactly matched the intended value.

`compensating`
: A write or readback failed and exact-original compensation is in progress.

`compensated`
: Exact original was restored and verified.

`relinquished`
: External drift or target disappearance transferred ownership away from
  optid; no overwrite was attempted.

Committed records remain until verified handback. A later intended value for
the same owned target refreshes the record while retaining the original
baseline. Records compact only after verified exact restoration or explicit
ownership relinquishment, and the recovery directory is synced after removal.

## Failure behavior

- Missing baseline: deny before prepare.
- Existing record from another generation: deny as stale generation.
- Canonical identity change or path reuse: deny without writing.
- Journal I/O failure: expose the recovery-journal gate denial.
- Target write failure: compensate to the captured original and verify.
- Readback mismatch or unavailable readback after a write: compensate and
  verify.
- Repeated compensation: succeed idempotently when the original is already
  present.
- External drift during handback: relinquish and compact without overwriting
  the external owner.
- Device removal during handback: validate the transaction generation, require
  every attribute of the target to return `NotFound`, and relinquish without a
  hardware write. Durably record relinquishment before compaction, then continue
  restoring the surviving targets. This is not reported as an exact restore.
  A partially missing runtime-PM target, changed identity, permission error, or
  journal failure still fails closed and retains the undo record.
- Compensation failure: retain the non-terminal record for S3D recovery and
  fail the control path visibly.

Legacy `original_*`, `intended_*`, and `applied_*` files are cleared only after
S2D compensation verifies the exact original. They remain migration inputs for
F4 until later packages remove the compatibility path.

## Verification matrix

The production enabled-versus-off matrix is also executed by `tools/checks.sh`
with the non-default `test-simulation` feature. Its device-removal regression
requires a planned topology restart, a completed handback before recovery runs,
and one clean supervised restart. Its configuration-reload regression must run
as well; compiling the feature alone does not execute either test.

The package tests cover:

- durable file and directory sync before the production write;
- full disk before publication;
- file-sync failure;
- partial publication/rename failure;
- the prepared-record crash window before a write;
- write failure and verified compensation;
- readback mismatch and verified compensation;
- stale generation;
- canonical path reuse;
- repeated compensation;
- external drift without overwrite;
- committed-record cleanup after verified restore;
- stable record naming across process generations and toolchain upgrades;
- stale-generation handback preserving recovery evidence;
- removed-device relinquishment with restoration of surviving controls;
- a partially missing runtime-PM target retaining its complete undo record;
- stale-generation rejection even when the target has disappeared;
- failed durable relinquishment preserving the original recovery record;
- process restart refusing handback before mutation until S3D recovers the
  previous generation;
- compensation continuing across every target after an earlier failure; and
- the real `run()` daemon path with persistent prepare/commit/restore/compact
  behavior.

## Builder proof

The clean candidate was produced by GitHub Actions run `30835980552`, job
`91761378250`, from staging head
`ae5b92cbad6e6226ea416eb9519bce7ce204d8f5`. The bounded builder passed:

- `cargo fmt --all -- --check`;
- `cargo check -p optid --all-targets --all-features`;
- `cargo clippy -p optid --all-targets --all-features -- -D warnings`;
- all thirteen mapped S2D acceptance tests individually with `--exact`;
- the complete `cargo test -p optid` suite;
- current-work, package-ledger, generated README, and whitespace validators;
  and
- `tools/finish-work.sh --dry-run`.

Only after those checks succeeded did the workflow remove every temporary
builder payload, workflow, and normalizer file and publish clean candidate
commit `a0f3179b7feb7e2a9d38bf43927e23998c7baf26`.

## Audited contract-repair proof

A subsequent audit found and corrected four package-boundary defects before
maintainer review:

- completed F2 proof files are byte-identical to `main` and are absent from the
  final S2D diff;
- stable record names use an explicit persisted hash instead of Rust's
  non-contractual `DefaultHasher`;
- stale-generation handback is rejected before mutation and preserves recovery
  evidence for S3D; and
- multi-target compensation attempts every target before returning the first
  error.

GitHub Actions run `30839184349`, job `91771771259`, tested the corrected source,
created a clean local candidate commit, and then validated that exact committed
snapshot before pushing it. The run passed:

- format, all-target compile, and warning-free clippy;
- all seventeen mapped S2D/F4-boundary acceptance tests individually with
  `--exact`;
- the complete `cargo test -p optid` suite;
- current-work, package-ledger, generated README, integrity, documentation,
  optid contract, evidence, and repository-policy checks; and
- `tools/finish-work.sh --dry-run`.

The validated clean candidate commit was
`8226724b565e304a167336966f8167c52e38d34a`. S2D remains `candidate`. Because
S2D changes declared F4 reconciliation proof paths, F4 is truthfully recorded
as `merged_incomplete` pending fresh independent cold verification of the
integrated F4/S2D surface. F1–F3 remain completed.

This is builder evidence, not an independent completion receipt. S2D must be
merged and then cold-verified by a separate verifier before it may become
`completed` or unlock S3D.

## Boundaries for later packages

S3D must add the minimal independent `optid-recover` executable, boot ordering,
and watchdog semantics. S4D must move mutation to pre-opened typed descriptors
and install Landlock. S5D must add persistent domain/HWID circuit state and
controlled canary re-entry. This package deliberately implements none of those
responsibilities.
