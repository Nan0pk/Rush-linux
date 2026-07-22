# Corrected Path Forward — Rush Linux v0.6 → v1.0

**Date:** 2026-07-19  
**Author:** Repository Analysis  
**Status:** Superseded for implementation sequencing; retained as a v0.6 release-evidence plan
**Supersedes:** Implicit path in `ROADMAP.md` (version sequencing correction)
**Superseded by:** `OPTID-COMPLETION-PLAN.md` and `docs/plans/optid-package-status.toml` for construction order

---

## Executive Summary

This document corrects milestone sequencing and identifies the blocker for **v0.6 Phase D evidence and release promotion**: nomination of two reference machines. It no longer defines the optid construction queue.

The project is currently at version `0.7.0-beta.4`, but this is a **versioning artifact** — v0.6.0-beta.1 (Hardware-Aware optid) remains incomplete with 0/4 exit criteria verified. Per the Evidence Rule, v0.7 (Editions) cannot be meaningfully completed until v0.6's quantitative criteria are satisfied, because edition behavior (desktop, laptop, realtime) must be validated against the hardware-aware optid policies.

**Recommendation:** Keep the version pointer at `0.7.0-beta.4` and do not make later release claims until v0.6 evidence closes. Continue safe construction through the active optid plan: F1 is the next general package and D0 is the next safety package.

---

## Critical Path Analysis

### Release and promotion gates (sequential dependencies)

```
v0.6 Phase A-C (in-container) → v0.6 Phase D (hardware) → v0.7 (editions) → v0.8 (benchmarks) → v0.9 (RC) → v1.0 (stable)
```

**Current release-evidence state:**
- ✅ v0.6 Phase A slice (allowlist verify + default flip + Criterion 4 harness) — merged; broader capability completion continues in the active plan
- ✅ v0.6 Phase B (PPD/GameMode shims + conflict detection) — merged (PRs #183-#186)
- ✅ v0.6 Phase C (foreground stub + vm.guest class) — merged (PRs #185-#186)
- ⛔ **v0.6 Phase D (hardware benchmarks)** — BLOCKED awaiting machine nomination
- ⏸️ v0.7 (editions) — cannot validate without v0.6 Phase D data
- ⏸️ v0.8+ — downstream of v0.7

### The v0.6 release-evidence blocker

**Action Required:** Project owner must nominate two physical reference machines per [`docs/strategy/reference-hardware.md`](../strategy/reference-hardware.md):

| Slot | Status | Action |
|------|--------|--------|
| Desktop | ❌ TBD | Nominate machine, confirm physical access, confirm baseline distro (Ubuntu 24.04 LTS recommended) |
| Laptop | ❌ TBD | Nominate machine (HP Victus candidate exists but needs clean re-capture), confirm battery present, confirm baseline distro |

**What this blocks:**
- v0.6 Criterion 2: "mixed-load responsiveness improves on two machines" — requires physical runs
- v0.6 Criterion 3: "battery behavior matches or improves mainstream defaults" — requires battery-equipped laptop
- v0.7 edition validation and later release promotion claims that depend on proven optid behavior

It does **not** block observation, simulation, dry-run, disabled implementation,
F1–F4, D0, diagnostics, or pure controllers. Those dependencies are defined in
`OPTID-COMPLETION-PLAN.md`, not this historical release sequence.

---

## Corrected Milestone Sequence

### Immediate Term (Next 2-4 Weeks)

#### 1. Project Owner Action — Machine Nomination (Week 1)
- [ ] Fill desktop slot in `docs/strategy/reference-hardware.md`
- [ ] Fill laptop slot in `docs/strategy/reference-hardware.md`
- [ ] Confirm both boards are seeded in `config/optid/hardware-allowlist.toml` (or add them)
- [ ] Schedule ~2 hour window for benchmark runs (baseline + optid × 2 machines × 2 runs each ≈ 4 runs × 30 min)

#### 2. v0.6 Phase D Execution (Week 2-3)
Once machines are nominated:
- [ ] D3: Run baseline (Ubuntu 24.04, PPD balanced) on both machines via `rush-host-bench.sh --submit`
- [ ] D4: Run optid (--apply) on both machines via `rush-host-bench.sh --submit`
- [ ] D5: Compare results, verify PASS conditions per `docs/strategy/mixed-load-workload.md`
- [ ] Commit transcripts to `release/evidence/host-bench/<date>-<hostname>/`
- [ ] Update `release/milestones.toml` v0.6 criteria_status rows to `verified = true` with transcript paths

#### 3. v0.6 Closure (Week 3-4)
- [ ] Verify Criterion 1 (unsupported knobs skipped) — needs host-bench transcript exercising skip path
- [ ] Verify Criterion 4 (no unsafe writes) — Criterion 4 harness test passes on real hardware
- [ ] Flip `--allowlist` default from `disabled` to `enabled` (per research 0006 §7)
- [ ] Mark v0.6.0-beta.1 status = "complete" in `release/milestones.toml`
- [ ] Run `python3 tools/dragnet.py --observe` — must return GREEN with v0.6 showing 4/4 verified

### Medium Term (Month 2-3)

#### v0.7.0-beta.1 — Editions (After v0.6 Complete)
- [ ] Implement mkosi profiles for desktop, laptop, server, realtime-audio
- [ ] Build sysexts for edition-specific configurations
- [ ] Validate each edition boots and applies correct optid policy
- [ ] Commit transcripts to `release/evidence/v0.7.0-beta.1/`
- [ ] Mark v0.7 status = "complete"

**Note:** Edition validation requires v0.6 Phase D data to prove editions behave correctly under hardware-aware optid.

#### v0.8.0-beta.1 — Benchmark Lab
- [ ] Integrate Phoronix Test Suite
- [ ] Automate benchmark artifact generation
- [ ] Wire regression detection to block RC promotion

### Long Term (Month 4-6)

#### v0.9.0-rc.1 — Release Candidate
- [ ] Freeze all public APIs and schemas
- [ ] Complete security review
- [ ] Sign all package metadata with production keys

#### v1.0.0 — Stable Release
- [ ] Publish all four editions
- [ ] Publish benchmark report
- [ ] Declare stable update channel

---

## Version Sequencing Correction

**Current issue:** `VERSION` file shows `0.7.0-beta.4` but v0.6 is incomplete.

**Root cause:** Version was advanced during edition scaffolding work, but v0.6's hardware-gated criteria were not yet satisfied.

**Correction:**
1. Keep version at `0.7.0-beta.4` (no rollback — honest signaling that we're past v0.6 code-wise)
2. Treat v0.6 completion as the **quality gate** before v0.7 edition validation
3. Do not advance to v0.8.0 until v0.6 shows 4/4 verified in Dragnet reports

**Honest communication:** The README badge and release notes should clarify:
> "v0.7.0-beta.4: Edition scaffolding is in place, but hardware-aware optid (v0.6) validation is pending physical-machine benchmarks. Next milestone: close v0.6 Phase D."

---

## Risk Register

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| No machines nominated within 2 weeks | HIGH | CRITICAL — v0.6 evidence and dependent release promotion stall | Preserve construction momentum through the active optid plan while the owner resolves hardware access |
| Machines nominated but not accessible for runs | MEDIUM | HIGH | Confirm access owner and scheduling commitment upfront |
| Baseline runs fail due to missing hardware support | MEDIUM | MEDIUM | Use allowlist override mechanism (`/etc/optid/allowlist.d/`) to enable testing on unseeded hardware |
| Optid shows no improvement or regression | LOW-MEDIUM | HIGH | This is valid scientific outcome — document findings, iterate on policy, do not suppress negative results |
| Battery testing impossible (no laptop with removable battery access) | LOW | HIGH | Borrow laptop, use cloud lab service, or partner with hardware reviewer |

---

## Success Metrics

**Leading indicators (process health):**
- [ ] `reference-hardware.md` has both slots filled by 2026-07-26
- [ ] First host-bench transcript committed by 2026-08-02
- [ ] Dragnet report shows v0.6 with 4/4 verified by 2026-08-09

**Lagging indicators (product health):**
- [ ] Mixed-load latency p99 improves ≥10% on both machines (Criterion 2)
- [ ] Battery energy/work-unit ≤ baseline on laptop (Criterion 3)
- [ ] No write occurs outside allowlist on either machine (Criterion 4)

---

## Communication Plan

### Internal (Project Owner)
- Weekly reassessment via `.github/workflows/reassess.yml` updates `docs/strategy/COMPASS.md`
- Dragnet reports (`release/evidence/dragnet/DRAGNET-*.md`) provide honest milestone state

### External (GitHub Visitors)
- Add "Why Rush Linux" paragraph to README contrasting with PPD/TLP/tuned
- Add `release/evidence/README.md` index (already exists — excellent)
- Publish first benchmark blog post when v0.6 Phase D completes

---

## Appendix: Work Package Breakdown

### v0.6 Phase D — Detailed Tasks

**D1: Reference Hardware Nomination** (project owner)
- Fill `docs/strategy/reference-hardware.md` desktop/laptop slots
- Confirm HWIDs in `config/optid/hardware-allowlist.toml`

**D2: Workload Definition** (agent)
- Already complete: `docs/strategy/mixed-load-workload.md` defines `mixed-load-001`
- Wire preset into `rushbench` if not already done

**D3: Baseline Runs** (project owner on nominated hardware)
```bash
# On each machine, fresh Ubuntu 24.04 LTS install
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/rush-host-bench.sh | sudo bash -s -- --tag baseline-$(hostname)
# Commits evidence PR automatically if GH_TOKEN set
```

**D4: Optid Runs** (project owner on same hardware)
```bash
# Same machine, after baseline completes
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/rush-host-bench.sh | sudo bash -s -- --apply --tag optid-$(hostname)
```

**D5: Verification** (verifier agent)
- Check both PRs show PASS verdicts
- Confirm transcripts committed to `release/evidence/host-bench/`
- Update `release/milestones.toml` with transcript paths
- Run `python3 tools/dragnet.py --observe` — must show v0.6 with 4/4 verified

---

## Conclusion

The project is in an unusually strong position for its stage:
- Evidence discipline is exemplary (Dragnet protocol working)
- Code quality is high (all tests passing, clippy clean)
- Documentation is comprehensive and honest

**The remaining blocker for v0.6 hardware evidence is owner/hardware access:** two machines must be nominated and benchmarked. That does not make the human the blocker for ongoing engineering.

Once v0.6 evidence closes, dependent release promotion can proceed. Meanwhile,
construction follows the active completion plan and package ledger.

**Recommendation:** Pursue machine nomination for evidence in parallel with F1
and D0. Do not stop safe, independently testable construction.
