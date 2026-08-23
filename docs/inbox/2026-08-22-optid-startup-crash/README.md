# `optid --apply` stops at startup: the circuit-breaker file poisons the recovery scan

Found 2026-08-22 on the nominated laptop slot, the first time `optid --apply`
ran as root on real hardware in this repository's history of committed evidence.
It blocks every optid-arm capture, including the short Criterion 1 skip-path run
this directory was originally opened for.

## What happens

```
optid: boot state — policy_load_state=ok allowlist_load_state=ok
       allowlist_gate=true apply_armed=true baseline_armed=false
optid: S4D capability sealing observe-only
optid: InvalidRecord: parse /var/lib/optid/recovery/circuits-v1.json:
       missing field `generation` at line 42 column 1
```

The daemon armed correctly (`apply_armed=true` once `tuned` was stopped) and then
stopped. It never wrote `status`, `status.json`, `actions.log`, or an audit
record, so no allowlist denial was captured and no cycle completed.

## Why

Two record types share one directory, and only one of them can be parsed there.

| Writer | Path | Schema |
|--------|------|--------|
| `crates/optid/src/circuit_breaker.rs:36` | `/var/lib/optid/recovery/circuits-v1.json` | circuit-breaker state, no `generation` |
| `crates/optid/src/recovery.rs:16` + `reconciler/transaction.rs:3` | `/var/lib/optid/recovery/` | `TransactionRecord`, `generation` **required** (`recovery.rs:70`) |

The transaction recovery scan reads every `*.json` in that directory. It finds
the circuit-breaker file, cannot deserialize it as a `TransactionRecord`, and
reports `InvalidRecord` — correctly, by its own contract: an unparseable
transaction record must never be silently skipped, because that is how a real
pending rollback would get lost.

So the failure is not in the scanner. It is that the circuit-breaker persists
its state *inside* the directory whose invariant is "every file here is a
transaction record". `/var/lib/optid/recovery/` was empty before this run — the
daemon wrote the file itself and then died on it. See
`recovery-dir-listing.txt`; the two `systemd-unit_user.slice_property_*.json`
files beside it are genuine transaction records with the field.

## Two candidate repairs

1. **Move the circuit file out of the transaction directory** — for example
   `/var/lib/optid/circuits-v1.json`, with a one-time migration of an existing
   file. Keeps the directory invariant intact, which is what makes
   `InvalidRecord` a meaningful safety signal. Changes a persisted path.
2. **Restrict the scan to the transaction naming scheme.** No path change, but
   it weakens the invariant: a corrupt record whose *name* is off would now be
   skipped rather than reported.

Repair 1 is the one that keeps the safety property. Both belong to the
circuit-breaker / S2D–S3D packages rather than to D2, so this is filed rather
than fixed.

## Files here

- `daemon-stderr.log` — the literal failure, as captured.
- `circuits-v1.json.sample` — the file the daemon wrote and then rejected.
- `recovery-dir-listing.txt` — the directory holding both record types.
- `state-dir-listing.txt` — `/run/optid` at the moment of failure.

`tuned` was stopped for the run and restarted afterwards; the machine was left
as found.
