# Rush Linux — COMPASS (Strategic Why/Where)

> Living document. Re-grounds the project in its **WHY** and checks for **drift**.
> Refreshed each reassessment cycle (weekly, or every 10 merges to `main`) by
> `.github/workflows/reassess.yml`. Dated point-in-time snapshots live under
> `docs/strategy/reassessments/`. Keep this doc honest and short; data is collected
> via the GitHub API, only judgment lives here.

**Last reassessment:** 2026-06-15 · **Verdict:** ON-TRACK (evidence complete, canon on `main`, still pre-visibility)

---

## 1. Origin arc — no-project → now

1. **No project / need felt.** Linux laptops leave power and responsiveness on the
   table versus Apple silicon: no integrated, event-driven power+performance
   orchestrator that adapts to workload the way macOS does on M-series hardware.
2. **Research.** Deep-dives into Apple's power stack and the Linux blind spots,
   plus external architecture/power-orchestrator reviews, established the theory
   for an event-driven, dual-loop `optid` design.
   _Cited: `docs/research/0001-apple-power-stack-analysis.md`, `0002-rush-linux-architecture-review.md`,
   `0003-unified-power-orchestrator-paper.md` (all on `main` as of 2026-06-15)._
3. **Project start.** Repo created 2026-05-25. A governed, evidence-first
   development model was adopted (builder/verifier/human roles; honest-claims
   policy; no claim without verifiable evidence — see `docs/how-rush-is-built.md`
   and `docs/agent-protocol.md`).
4. **Canon set.** `docs/SPEC-northstar.md` fixed the single objective function,
   the lever ledger, the actuation rule, and the WP→evidence mapping; the agent
   anti-pivot contract guards against scope drift.
   _Cited: `docs/SPEC-northstar.md`, `docs/agent-protocol.md` (both on `main`)._
5. **Now.** Both SPEC §6 evidence gates are GREEN:
   - **LATENCY gate: CLOSED** (PR #72 merged — latency evidence dataset).
   - **ENERGY gate: CLOSED** (PR #75 merged — code + real mock-free battery run
     evidence at commit `557673d`; PR #79 trim landed at `4eabb42`).

## 2. New research / findings / what's been MISSED

- **Canon is now on `main`** (was the gap last cycle): research docs (0001/0002/0003),
  SPEC-northstar, and the agent protocol addendum are all visible in the trunk.
- **First complete evidence story:** latency + energy, both with mock-free
  measurements, both via the `rushbench` harness. Caveat: the energy dataset
  used a **dummy `optid --apply` shim** — proves the *probe*, not optid efficacy.
  Real optid-applied runs are a follow-up WP.
- **Energy measurement reality** (carried forward): short cells (~10 s) are
  shorter than a battery controller's `energy_now` update interval (10–60 s);
  RAPL is the reliable short-window counter. The 30 s Phase-2 sampling window +
  RAPL-priority detection closed this for `victus`.
- **All-zero `samples` array** in psi-cpu/psi-io (4.4 M–4.5 M entries × 0):
  likely a sampler bug. Energy data is unaffected (independent of `samples`),
  but a follow-up issue is the honest move. **NOT YET FILED.**
- **Watch next cycle:** sched_ext default-on (ADR 0015) implementation status;
  whether any installable/demoable artifact exists for outside users.

## 3. Third-person product outlook

**(a) Human user's view.** Today there is **no installable image or demo** a user
can run; the visible surface is a research+infra repo. The promise is
compelling but not yet *experienceable*. The new asset — complete evidence —
is **renderable**: latency + energy datasets can be charted in the README in
~1 hour. Closing this is the highest-leverage unblocker for human visibility.

**(b) AI-agent / contributor's view.** **Improved materially.** Two new
agent-legible assets on `main`:
- `.agents/skills/agent-bus/` — multi-agent protocol stashed for later reuse
  (currently SUSPENDED in solo mode).
- `.agents/skills/yagni-ladder/` — narrow extraction of ponytail's 6-rung
  decision tree, opt-in only, doesn't override the WP evidence rule.
Canon is on `main`, docmap is stable, doc-sync validator passes. A new agent
can orient in minutes.

## 4. Reach / visibility

| Metric | 2026-06-14 | 2026-06-15 | Δ |
|---|---|---|---|
| Stars | 1 | 1 | — |
| Forks | 0 | 0 | — |
| Open issues | 30 | 26 | −4 |
| Open PRs | 3 | 0 | −3 (all merged) |
| Traffic views (14d) | n/a | 195 (4 unique) | first cycle |
| Traffic clones (14d) | n/a | 2,374 (571 unique) | first cycle |
| Repo age | ~3 weeks | ~3 weeks | — |
| Repo size | ? | 1,833 KB | (post-trim: 311 MB → 12 KB on benchmark results) |

**Interpretation:** 571 unique clones vs 4 unique views is a strong signal
that **agents/CI are engaging with this repo**, not humans. The visibility
gap is on the human side, not the technical side. Rendered evidence in the
README would close it.

## 5. Verdict & course-corrections

**Verdict: ON-TRACK.** Canon on `main`, evidence complete, agent-legibility
improved. **Still pre-visibility** — no rendered proof, no installable
artifact, low star count.

**Course-corrections (1–3):**
1. **Make the proof visible.** Render the latency + energy datasets in
   `README.md` (chart or short "what this proves" section). Effort: ~1 hour.
   Highest-leverage unblocker for human visibility.
2. **File the all-zero `samples` sampler-bug issue** (caution #3 from the
   WP-ENERGY-PROBE verdict). Effort: ~30 min to file.
3. **Triage the new skills.** `.agents/skills/yagni-ladder/` is opt-in and won't
   fire unless a verifier flags over-engineering; confirm a verifier uses it
   on the next non-trivial PR. `.agents/skills/agent-bus/` is SUSPENDED — keep
   it that way until multi-agent mode is re-engaged.

---

_How this doc is maintained: each cycle, `reassess.yml` opens/updates a
"Strategic Reassessment" issue with auto-collected metrics + the checklist; a
human or agent fills the judgment sections, updates this file, drops a dated
snapshot in `docs/strategy/reassessments/`, and posts the verdict as an issue
comment. Snapshots are append-only data and are not individually registered in
`docs/docmap.toml`._
