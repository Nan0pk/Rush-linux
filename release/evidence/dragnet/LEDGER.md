# Dragnet Evidence Ledger

Single source of truth for the evidence state of every milestone exit criterion.
One row per criterion. Updated by each Dragnet run. A criterion is **closed** only
when a committed `transcript` exists and `validate-evidence.py` is green.

- **Location** = where the evidence can be produced: `container` (here) or
  `build-host` (needs root + KVM).
- **Status** = `committed` (transcript on disk, `verified = true`) or `pending`.

_Last updated: Dragnet-001, 2026-06-22._

| ID | Milestone | Criterion | Location | Status | Transcript |
|----|-----------|-----------|----------|--------|------------|
| v0.3.1 | 0.3.0-alpha.1 | minimal VM boots to multi-user.target | build-host | pending | — |
| v0.3.2 | 0.3.0-alpha.1 | cgroup v2 and PSI are active | build-host | pending | — |
| v0.3.3 | 0.3.0-alpha.1 | optid.service starts | build-host | pending | — |
| v0.3.4 | 0.3.0-alpha.1 | nftables.conf loads | build-host | pending | — |
| v0.4.1 | 0.4.0-alpha.1 | VM boots through UKI | build-host | pending | — |
| v0.4.2 | 0.4.0-alpha.1 | three rollback entries are retained | build-host | pending | — |
| v0.4.3 | 0.4.0-alpha.1 | simulated bad kernel rolls back | build-host | pending | — |
| v0.4.4 | 0.4.0-alpha.1 | test update metadata is signed | container | **committed** | `v0.4.0-alpha.1/c4-update-signed/transcript.log` |
| v0.5.1 | 0.5.0-beta.1 | fresh VM install succeeds | build-host | pending | — |
| v0.5.2 | 0.5.0-beta.1 | installed system boots twice cleanly | build-host | pending | — |
| v0.5.3 | 0.5.0-beta.1 | update and rollback tests pass | build-host | pending | — |
| v0.5.4 | 0.5.0-beta.1 | server edition has no desktop dependency | container | **committed** (static) | `v0.5.0-beta.1/c4-server-no-desktop/transcript.log` |

## Summary

- **Closed with committed transcript:** 2 / 12 (v0.4.4 signing; v0.5.4 server-no-desktop, static).
- **Pending build-host transcript:** 10 / 12 — see `release/evidence/BUILD-HOST-RUNBOOK.md`.
- **Milestones at `complete`:** 0. v0.3 / v0.4 / v0.5 are `evidence-pending` until their rows close.

## Notes

- v0.5.4 is closed by a static analysis of the *declared* package set; the
  build-host runbook adds a built-image dependency-closure check as confirmation.
- Earlier `verified = true` flags on the 10 pending rows were set without committed
  transcripts (Dragnet-001 findings G1/G3/G5) and have been reset to `false`.
- v0.1 / v0.2 have no `criteria_status` rows; their "complete" rests on CI history
  (compile/test), supported by the snapshot at `release/evidence/core-tests/`.
