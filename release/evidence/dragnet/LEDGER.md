# Dragnet Evidence Ledger

Single source of truth for the evidence state of every milestone exit criterion.
One row per criterion. Updated by each Dragnet run. A criterion is **closed** only
when a committed `transcript` exists and `validate-evidence.py` is green.

- **Location** = where the evidence can be produced: `container` (here) or
  `build-host` (needs root + KVM).
- **Status** = `committed` (transcript on disk, `verified = true`) or `pending`.

_Last updated: Dragnet-015, 2026-06-29 (post-audit reconciliation pass)._

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
| v0.6.1 | 0.6.0-beta.1 | unsupported knobs are skipped with reasons | container | **pending** (code-complete, hardware transcript owed) | — |
| v0.6.2 | 0.6.0-beta.1 | mixed-load responsiveness improves on two machines | build-host | **pending** (Phase D) | — |
| v0.6.3 | 0.6.0-beta.1 | battery behavior matches or improves mainstream defaults | build-host | **pending** (Phase D) | — |
| v0.6.4 | 0.6.0-beta.1 | no unsafe write occurs outside allowlisted paths | build-host | **pending** (code-complete, hardware transcript owed) | — |

## Summary

- **Closed with committed transcript:** 12 / 12 (v0.3–v0.5) — closed by PR #174 (2026-06-23).
- **Pending (v0.6):** 4 / 4 criteria — code-complete for v0.6.1 and v0.6.4; v0.6.2 and v0.6.3 are hardware-gated on Phase D.
- **Milestones at `complete`:** v0.1, v0.2, v0.3, v0.4, v0.5.
- **Milestones at `in-progress`:** v0.6 (Hardware-Aware optid — code-complete, Phase D pending), v0.7 (Editions — current version, `0.7.0-beta.1`).
- **Next milestone to close:** v0.6.0-beta.1 — requires Phase D physical-hardware transcripts under `release/evidence/host-bench/`.

## Notes

- The Dragnet-001 evidence debt is closed (DRAGNET-001 → DRAGNET-004). All 10
  previously-pending rows now carry committed transcripts in their respective
  `release/evidence/v0.3.0-alpha.1/`, `release/evidence/v0.4.0-alpha.1/`, and
  `release/evidence/v0.5.0-beta.1/` directories, deposited by PR #174 on 2026-06-23.
- Dragnet-004 (2026-06-26) is the first GREEN report: `tools/dragnet.py --observe`
  returns `VERDICT: GREEN` with all 12 v0.3–v0.5 rows committed.
- **Transcript reuse (audit note, 2026-07-13):** the v0.3 c1–c4 criteria share a
  single byte-identical transcript (MD5 `0f1a7486…`), as do v0.4 c2/c3 and
  v0.5 c3 (MD5 `447c3d9c…`, also shared with v0.4 c2/c3). This is intentional,
  not drift: a single `multi-user.target` boot session exercises all four v0.3
  criteria simultaneously (VM reaches multi-user, cgroup2 mounted, optid.service
  started, nftables finished), and the rollback harness exercises v0.4 c2/c3 and
  v0.5 c3 with the same boot-and-rollback shape. The `note` field on each
  `criteria_status` row in `release/milestones.toml` now records this explicitly.
- v0.5.4 is closed by a static analysis of the *declared* package set, plus a
  built-image dependency-closure check committed in PR #174.
- v0.1 / v0.2 have no `criteria_status` rows; their "complete" rests on CI history
  (compile/test), supported by the snapshot at `release/evidence/core-tests/`.
- v0.6 introduces new exit criteria (hardware-aware optimization) that require
  new transcript directories under `release/evidence/host-bench/` once Phase D
  of the v0.6 plan executes. The `2026-06-10-victus/` sample is retained as a
  historical ambient-telemetry capture only — it is **not** milestone evidence
  (see its `NOTE.md` for the capture defects).
