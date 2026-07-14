# Rush Linux — COMPASS (Strategic Why/Where)

> Living document. Re-grounds the project in its **WHY** and checks for **drift**.
> Refreshed each reassessment cycle (weekly, or every 10 merges to `main`) by
> `.github/workflows/reassess.yml`. Dated point-in-time snapshots live under
> `docs/strategy/reassessments/`. Keep this doc honest and short; data is collected
> via the GitHub API, only judgment lives here.

**Last reassessment:** 2026-06-26 · **Verdict:** ON-TRACK (Dragnet-001 evidence debt closed by PR #174; v0.3/v0.4/v0.5 genuinely complete with committed transcripts; v0.6 implementation proposal in PR #175; v0.6 Phase D hardware-benchmark cycle is the next hard gate)

---

## 1. Origin arc — no-project → now

1. **No project / need felt.** Linux laptops leave power and responsiveness on the
   table versus Apple silicon: no integrated, event-driven power+performance
   orchestrator that adapts to workload the way macOS does on M-series hardware.
2. **Research.** Deep-dives into Apple's power stack and the Linux blind spots,
   plus external architecture/power-orchestrator reviews, established the theory
   for an event-driven, dual-loop `optid` design.
   _Cited: `docs/research/0001-apple-power-stack-analysis.md`, `0002-rush-linux-architecture-review.md`,
   `0003-unified-power-orchestrator-paper.md` (all on `main`)._
3. **Project start.** Repo created 2026-05-25. A governed, evidence-first
   development model was adopted (builder/verifier/human roles; honest-claims
   policy; no claim without verifiable evidence — see `docs/how-rush-is-built.md`
   and `docs/agent-protocol.md`).
4. **Canon set.** `docs/SPEC-northstar.md` fixed the single objective function,
   the lever ledger, the actuation rule, and the WP→evidence mapping; the agent
   anti-pivot contract guards against scope drift.
5. **Specification Issue Retirement (ADR 0016).** Migrated long-term Work Package specifications and Track epics out of open GitHub issues, consolidating them natively within the documentation canon to establish a strict semantic rule for issues.
6. **Now.** The Core MVP control plane (`optid`, `optctl`) is 100% compile-clean and robustly hardened (single-instance state `flock`, signal hooks for guaranteed systemd reversion, Path directory traversal blocks). The **v0.5 Image Pivot is complete** — PR #174 (2026-06-23) closed the Dragnet-001 evidence debt and committed all 12 outstanding v0.3/v0.4/v0.5 build-host acceptance transcripts. `release/milestones.toml` is the canonical, honest source of truth; Dragnet-004 (2026-06-26) is the first GREEN report. The v0.6 implementation proposal (PR #175, `docs/plans/v0.6-hardware-aware-optid-proposal.md`) targets a hardware allowlist, PPD/GameMode shims, foreground detection, a `vm.guest` class, and the first physical-machine benchmarks.

## 2. Deep Subsystem Audit & Core Validation

- **Actuator Hardening Complete:** Resolved high-priority correctness and security RFCs (PR #100, #101). `optid` actively acquires an exclusive `flock` on `optid.lock`, registers signal hooks (`SIGTERM`, `SIGINT`, `SIGHUP`) to break its loop cleanly so `revert_sysctls` and `revert_pm_qos` are deterministically invoked, and structurally blocks directory traversal (`..`) in `guarded_write`.
- **TOML Crate Refactoring:** Replaced over 110 lines of repetitive manual TOML parsing in `Policy::load` with the canonical `toml` crate.
- **100% Test & Doc Verification Suite Green:** All 50 pure-Rust workspace tests pass. `validate-doc-sync.py` confirms flawless cross-references.
- **Dragnet-001 Closed (2026-06-23):** PR #174 committed the 10 outstanding build-host acceptance transcripts for v0.3/v0.4/v0.5. Dragnet-004 (2026-06-26) is the first GREEN report. The earlier "evidence-pending" status on v0.3/v0.4/v0.5 — flagged by Dragnet-001 because `verified = true` flags lacked committed transcripts — is now resolved honestly.

## 3. Third-person product outlook

**(a) Human user's view.** The product is still pre-consumer. The closest a human gets to "using" Rush today is booting the mkosi-built image in QEMU. The v0.5 closure means the install path is real (not just "boots in CI"): `tools/rush-install.sh` writes the OS image onto a blank disk via `systemd-repart`, the installed system boots twice cleanly with `optid.service` active, and bad-kernel rollback is verified end-to-end. There is no desktop, no installer GUI, and no hardware-tuned behavior yet — those land in v0.6 (hardware-aware optid) and v0.7 (desktop edition).

**(b) AI-agent / contributor's view.** **Pristine.** The Dragnet protocol, Builder/Verifier split, docmap, and `start-work.sh` / `finish-work.sh` lifecycle make this one of the more agent-friendly repos in its weight class. The v0.6 proposal (PR #175) breaks Phase A/B/C into in-container Work Packages that an agent can execute without hardware — only Phase D needs the project owner's hands. One open issue (#105, this reassessment cycle) and two open docs PRs (#175, #176).

## 4. Reach / visibility

| Metric | 2026-06-17 | 2026-06-26 | Δ |
|---|---|---|---|
| Stars | 1 | 1 | — |
| Forks | 0 | 0 | — |
| Open issues | 1 | 1 | — |
| Open PRs | 0 | 2 (#175, #176) | +2 |
| Traffic clones | 2,450 | n/a (token-scoped) | — |

**Interpretation:** The incredibly focused, zero-clutter GitHub tracking surface still communicates rigorous engineering discipline to visiting auditors. However, 1 star / 0 forks after a month of substantive v0.5 work indicates the project has **no external visibility**. The README describes *what* Rush is but not *why* a user should switch from PPD/TLP/tuned. The `release/evidence/` tree is a model of evidence-driven development but is invisible to casual GitHub visitors — a `release/evidence/README.md` index would close that gap.

## 5. Verdict & course-corrections

**Verdict: ON-TRACK.** The project closed its largest internal risk (Dragnet-001) this cycle. v0.3/v0.4/v0.5 are genuinely complete with committed transcripts; Dragnet-004 is GREEN. The roadmap from v0.6 to v1.0 is concrete and the implementation plans are at the right level of detail. The two real risks — hardware availability for v0.6 Phase D and discoverability for the project overall — are both outside the codebase and addressable in parallel with code work.

**Course-corrections for the v0.6 Push:**
1. **Nominate the two v0.6 reference machines this week.** This is the single action that prevents v0.6 from stalling. The v0.6 proposal (`docs/plans/v0.6-hardware-aware-optid-proposal.md` §10) lists the open questions; the project owner's answer unblocks Phase D.
2. **Add a `release/evidence/README.md` index.** One file, one table, one afternoon. Makes the evidence tree visible to GitHub visitors without changing any protocol. Pair with a short "Why Rush Linux" paragraph in the main README that contrasts with PPD/TLP/tuned.
3. **Time a "first public benchmark" post to v0.6 Phase D completion.** The first published `rushbench` results — even on two machines — are the project's strongest possible external signal. Draft the blog post / HN submission text in parallel with Phase D so it ships the same day the transcripts land.

---

_How this doc is maintained: each cycle, `reassess.yml` opens/updates a
"Strategic Reassessment" issue with auto-collected metrics + the checklist; a
human or agent fills the judgment sections, updates this file, drops a dated
snapshot in `docs/strategy/reassessments/`, and posts the verdict as an issue
comment. Snapshots are append-only data and are not individually registered in
`docs/docmap.toml`._
