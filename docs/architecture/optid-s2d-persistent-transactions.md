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
- Compensation failure: retain the non-terminal record for S3D recovery and
  fail the control path visibly.

Legacy `original_*`, `intended_*`, and `applied_*` files are cleared only after
S2D compensation verifies the exact original. They remain migration inputs for
F4 until later packages remove the compatibility path.

## Verification matrix

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
- committed-record cleanup after verified restore; and
- the real `run()` daemon path with persistent prepare/commit/restore/compact
  behavior.

## Boundaries for later packages

S3D must add the minimal independent `optid-recover` executable, boot ordering,
and watchdog semantics. S4D must move mutation to pre-opened typed descriptors
and install Landlock. S5D must add persistent domain/HWID circuit state and
controlled canary re-entry. This package deliberately implements none of those
responsibilities.
