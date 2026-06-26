# Dragnet Evidence Ledger

Single source of truth for the evidence state of every milestone exit criterion.
One row per criterion. Updated by each Dragnet run. A criterion is **closed** only
when a committed `transcript` exists and `validate-evidence.py` is green.

- **Location** = where the evidence can be produced: `container` (here) or
  `build-host` (needs root + KVM).
- **Status** = `committed` (transcript on disk, `verified = true`) or `pending`.

_Last updated: Dragnet-004, 2026-06-26._

| ID | Milestone | Criterion | Location | Status | Transcript |
|----|-----------|-----------|----------|--------|------------|
| v0.3.1 | 0.3.0-alpha.1 | minimal VM boots to multi-user.target | build-host | **committed** | `v0.3.0-alpha.1/c1-multiuser/transcript.log` |
| v0.3.2 | 0.3.0-alpha.1 | cgroup v2 and PSI are active | build-host | **committed** | `v0.3.0-alpha.1/c2-cgroup-psi/transcript.log` |
| v0.3.3 | 0.3.0-alpha.1 | optid.service starts | build-host | **committed** | `v0.3.0-alpha.1/c3-optid-service/transcript.log` |
| v0.3.4 | 0.3.0-alpha.1 | nftables.conf loads | build-host | **committed** | `v0.3.0-alpha.1/c4-nftables/transcript.log` |
| v0.4.1 | 0.4.0-alpha.1 | VM boots through UKI | build-host | **committed** | `v0.4.0-alpha.1/c1-uki-boot/transcript.log` |
| v0.4.2 | 0.4.0-alpha.1 | three rollback entries are retained | build-host | **committed** | `v0.4.0-alpha.1/c2-rollback-retain/transcript.log` |
| v0.4.3 | 0.4.0-alpha.1 | simulated bad kernel rolls back | build-host | **committed** | `v0.4.0-alpha.1/c3-bad-kernel/transcript.log` |
| v0.4.4 | 0.4.0-alpha.1 | test update metadata is signed | container | **committed** | `v0.4.0-alpha.1/c4-update-signed/transcript.log` |
| v0.5.1 | 0.5.0-beta.1 | fresh VM install succeeds | build-host | **committed** | `v0.5.0-beta.1/c1-fresh-install/transcript.log` |
| v0.5.2 | 0.5.0-beta.1 | installed system boots twice cleanly | build-host | **committed** | `v0.5.0-beta.1/c2-double-boot/transcript.log` |
| v0.5.3 | 0.5.0-beta.1 | update and rollback tests pass | build-host | **committed** | `v0.5.0-beta.1/c3-update-rollback/transcript.log` |
| v0.5.4 | 0.5.0-beta.1 | server edition has no desktop dependency | container | **committed** (static) | `v0.5.0-beta.1/c4-server-no-desktop/transcript.log` |

## Summary

- **Closed with committed transcript:** 12 / 12 — closed by PR #174 (2026-06-23).
- **Pending build-host transcript:** 0 / 12.
- **Milestones at `complete`:** v0.1, v0.2, v0.3, v0.4, v0.5.
- **Next milestone:** v0.6.0-beta.1 (Hardware-Aware optid) — see
  `docs/plans/v0.6-hardware-aware-optid-proposal.md`.

## Notes

- The Dragnet-001 evidence debt is closed (DRAGNET-001 → DRAGNET-004). All 10
  previously-pending rows now carry committed transcripts in their respective
  `release/evidence/v*/*` directories, deposited by PR #174 on 2026-06-23.
- Dragnet-004 (2026-06-26) is the first GREEN report: `tools/dragnet.py --observe`
  returns `VERDICT: GREEN` with all 12 rows committed.
- v0.5.4 is closed by a static analysis of the *declared* package set, plus a
  built-image dependency-closure check committed in PR #174.
- v0.1 / v0.2 have no `criteria_status` rows; their "complete" rests on CI history
  (compile/test), supported by the snapshot at `release/evidence/core-tests/`.
- v0.6 introduces new exit criteria (hardware-aware optimization) that will
  require new transcript directories under `release/evidence/host-bench/`
  once Phase D of the v0.6 plan executes.
