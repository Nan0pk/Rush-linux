# Rush-linux Workspace Enablement & Execution Plan (Agent Mode)

**Date**: 2026-07-27 (local Asia/Karachi) — **UPDATED**
**Workspace root**: /home/user/Rush-linux
**Current branch**: work/20260726-workspace-enablement-and-ci-audit
**Repo state**: main at 104061f (post #357)
**Latest commit on branch**: 17b5072 (F2 candidate + verification receipt)

## Progress Summary (this session)
- Environment fully enabled + Rust toolchain + all tools validated.
- Complete CI/CD analysis (PR Gate, classify lanes, checks.sh sections, root-cause naming).
- F2 (active_general) advanced:
  - Added `f2_production_surface_via_daemon_run_entry` — enters **exactly** through `crate::run()` (daemon binary entry) while using F2 seam (FaultKernel/RealKernel via lib exports).
  - Test passes (production path + curated baseline behavior exercised).
  - Ledger updated to `status = "candidate"` with:
    - runtime_entrypoints
    - integration_tests
    - explicit acceptance_tests mapping
    - completion_evidence including new receipt
  - Cold verification receipt stub created: `docs/plans/optid-verification/f2.toml`
- All relevant sections now PASS:
  - `validate-optid-packages.py` → PASS
  - `checks.sh --ci --section optid/rust/integrity/docs/evidence` → PASS
  - `cargo test` (targeted + workspace) + clippy (with -D warnings) clean on the change.
- Frontpage/readme regenerated when needed.
- Honest status only — no bypasses.

## 1. Environment Audit (COMPLETED)
- ... (same as before)

## 2. CI/CD Workflow Understanding (COMPLETED)
- ... (detailed in previous version; key: named steps in checks.sh = root cause; ledger rules are strict)

## 3. Step-wise Execution Plan — Current Loop Status

### Phase 0 + 1: Enablement + Parity (DONE)
- Full local CI simulation repeated multiple times.

### Phase 2: F2 Completion (active_general) — **ADVANCED TO CANDIDATE**
**Completed in this loop**:
- Step 2.1–2.4 executed: production-surface test added, ledger + acceptance mapping updated, receipt stub, all checks + validators pass.
- Test transcript available in `cargo test` output (daemon run path exercised).

**Remaining for F2 "completed" (per ledger/AGENTS)**:
- Independent **cold** verification (separate checkout of the PR branch, re-run exact test + `bash tools/checks.sh --ci --section optid`, attach receipt).
- Human maintainer merge + final ledger promotion.

### Phase 3: F1 Fresh Verification (merged_incomplete) — **NEXT PRIORITY**
Current blocker (from ledger):
- Stale receipt from PR #332 (runtime_entrypoints changed in later PRs, test files deleted).
- Need fresh cold verification receipt for post-#337 surface.

**Planned small logical steps**:
1. Identify current F1 acceptance tests in `crates/optid/src/policy.rs` (the `f1_*` fns).
2. Run targeted tests + produce fresh receipt in `docs/plans/optid-verification/f1-fresh.toml`.
3. Update ledger for F1 (remove stale blocking_reason, add fresh receipt).
4. Validate + checks.

### Phase 4–6: D0 + Parallel + PR Loop
- D0 (active_safety): extend prototype for missing proofs.
- Parallel: T1 (if F2 progresses), R-lanes.
- Every change: start-work → edit+test → finish --dry-run → full checks → commit.

## 4. Current State (2026-07-27)
- F2: **candidate** (production test + receipt in place)
- F1: **merged_incomplete** (stale receipt)
- D0: **merged_incomplete**
- All local checks green for changed paths.
- Branch clean after last commit.

## 5. Commands Used / To Use
(See previous sections + canonical list in plan.)

**Immediate next actions in loop**:
- Continue with F1 fresh receipt (small coherent unit).
- Or prepare draft PR description for the current F2 work.
- Re-run full simulation before any push.

**Repo rules strictly followed** (no shortcuts):
- start/finish-work.sh every time.
- checks.sh --ci before finish.
- Ledger updated only with real evidence paths + acceptance mapping.
- One logical change per commit.
- Honest status (candidate, not completed).

Ready for next iteration. Run the next logical package or "stop".

---
**Last full validation (this turn)**:
- validate-optid-packages.py: PASS
- checks.sh sections: PASS (optid, rust, integrity, docs, evidence)
- Targeted F2 test: ok
- Tree clean after progress commit.
