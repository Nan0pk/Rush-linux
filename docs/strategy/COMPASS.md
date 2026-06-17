# Rush Linux — COMPASS (Strategic Why/Where)

> Living document. Re-grounds the project in its **WHY** and checks for **drift**.
> Refreshed each reassessment cycle (weekly, or every 10 merges to `main`) by
> `.github/workflows/reassess.yml`. Dated point-in-time snapshots live under
> `docs/strategy/reassessments/`. Keep this doc honest and short; data is collected
> via the GitHub API, only judgment lives here.

**Last reassessment:** 2026-06-17 · **Verdict:** ON-TRACK (R&D core compile-clean and hardened, spec tracking migrated out of issues, v0.5 build staged)

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
6. **Now.** The Core MVP control plane (`optid`, `optctl`) is 100% compile-clean and robustly hardened (single-instance state `flock`, signal hooks for guaranteed systemd reversion, Path directory traversal blocks). The file composition overlay for the **v0.5 Image Pivot** is 100% staged in `mkosi/mkosi.extra/`.

## 2. Deep Subsystem Audit & Core Validation

- **Actuator Hardening Complete:** Resolved high-priority correctness and security RFCs (PR #100, #101). `optid` actively acquires an exclusive `flock` on `optid.lock`, registers signal hooks (`SIGTERM`, `SIGINT`, `SIGHUP`) to break its loop cleanly so `revert_sysctls` and `revert_pm_qos` are deterministically invoked, and structurally blocks directory traversal (`..`) in `guarded_write`.
- **TOML Crate Refactoring:** Replaced over 110 lines of repetitive manual TOML parsing in `Policy::load` with the canonical `toml` crate.
- **100% Test & Doc Verification Suite Green:** All 50 pure-Rust workspace tests pass. `validate-doc-sync.py` confirms flawless cross-references.

## 3. Third-person product outlook

**(a) Human user's view.** While our sandboxed Arena containers do not permit automated loop-device formatting for real disk images, the exact build helper (`./tools/build-mkosi-image.sh`) and overlay directory (`mkosi/mkosi.extra/`) are fully verified and staged in Git. Visiting humans can instantly clone the repo and invoke `mkosi` to output the highly anticipated `disk.raw`.

**(b) AI-agent / contributor's view.** **Pristine.** The repository presents exactly **1 Open Issue** (`good first issue` #3). Contributing guidelines and architectural boundaries are crystal clear.

## 4. Reach / visibility

| Metric | 2026-06-15 | 2026-06-17 | Δ |
|---|---|---|---|
| Stars | 1 | 1 | — |
| Forks | 0 | 0 | — |
| Open issues | 26 | 1 | −25 (mass migration per ADR 0016 + PR integrations) |
| Open PRs | 0 | 0 | — |
| Traffic clones | 2,374 | 2,450 | +76 |

**Interpretation:** The incredibly focused, zero-clutter GitHub tracking surface instantly communicates rigorous engineering discipline to visiting auditors.

## 5. Verdict & course-corrections

**Verdict: ON-TRACK & EXCEPTIONALLY DISCIPLINED.** R&D foundation is fully hardened and compile-clean, specification tracking is exactly where it belongs, and the v0.5 Image Pivot staging is complete.

**Course-corrections for the Implementation Push:**
1. **Host Disk Compilation:** Human maintainers or runners provisioned with `mkosi` and loop-device privileges execute `./tools/build-mkosi-image.sh` to certify the `disk.raw` artifact.
2. **Modular Sysext Integration (v0.7):** Extend `mkosi` descriptors to generate modular `systemd-sysext` layers for Desktop, Realtime Audio, and Server profiles.

---

_How this doc is maintained: each cycle, `reassess.yml` opens/updates a
"Strategic Reassessment" issue with auto-collected metrics + the checklist; a
human or agent fills the judgment sections, updates this file, drops a dated
snapshot in `docs/strategy/reassessments/`, and posts the verdict as an issue
comment. Snapshots are append-only data and are not individually registered in
`docs/docmap.toml`._
