# Rush Linux — COMPASS (Strategic Why/Where)

> Living document. Re-grounds the project in its **WHY** and checks for **drift**.
> Refreshed each reassessment cycle (weekly, or every 10 merges to `main`) by
> `.github/workflows/reassess.yml`. Dated point-in-time snapshots live under
> `docs/strategy/reassessments/`. Keep this doc honest and short; data is collected
> via the GitHub API, only judgment lives here.

**Last reassessment:** 2026-06-14 · **Verdict:** ON-TRACK (pre-visibility, R&D phase)

---

## 1. Origin arc — no-project → now

1. **No project / need felt.** Linux laptops leave power and responsiveness on the
   table versus Apple silicon: no integrated, event-driven power+performance
   orchestrator that adapts to workload the way macOS does on M-series hardware.
2. **Research.** Deep-dives into Apple's power stack and the Linux blind spots,
   plus external architecture/power-orchestrator reviews, established the theory
   for an event-driven, dual-loop `optid` design.
   _Cited: PR #50 (Apple power stack analysis), PR #51 (architecture & power
   orchestrator reviews)._
3. **Project start.** Repo created 2026-05-25. A governed, evidence-first
   development model was adopted (builder/verifier/human roles; honest-claims
   policy; no claim without verifiable evidence — see `docs/how-rush-is-built.md`
   and `docs/agent-protocol.md`).
4. **Canon set.** `docs/SPEC-northstar.md` fixed the single objective function,
   the lever ledger, the actuation rule, and the WP→evidence mapping; the agent
   anti-pivot contract guards against scope drift.
   _Cited: PR #52 (SPEC-northstar + agent protocol)._
5. **Now.** Building the **evidence harness** (`rushbench`) and closing SPEC §6
   acceptance gates with real, mock-free measurements:
   - **LATENCY gate: CLOSED** (PR #72 merged — latency evidence dataset).
   - **ENERGY gate: OPEN** — code is clear (PR #75, CI green @ `b8fb799`); closes
     only on a real `rushbench matrix` run on battery producing `avg_watts > 0`.

## 2. New research / findings / what's been MISSED

- **Landed/active research** not yet on `main`: PRs #50, #51, #52 are open. The
  architecture canon and northstar spec are therefore not yet merged into the
  trunk — a contributor reading `main` cannot see the formal objective function.
  **MISSED:** merge or explicitly close these so the canon is discoverable.
- **Energy measurement reality:** short benchmark cells (~10 s) are shorter than a
  battery controller's `energy_now` update interval (10–60 s); RAPL is the
  reliable short-window counter. This shaped the PR #75 30 s sampling window +
  RAPL-priority detection. (Open item: confirm on real battery.)
- **Watch next cycle:** sched_ext default-on (ADR 0015) status; whether any
  installable/demoable artifact exists for outside users.

## 3. Third-person product outlook

**(a) Human user's view.** Today there is **no installable image or demo** a user
can run; the visible surface is a research+infra repo. The promise ("Apple-class
adaptive power/perf on Linux laptops") is compelling but not yet *experienceable*.
Conversion to interest depends on a runnable artifact or a vivid evidence story
(e.g., the latency dataset rendered as a chart in the README).

**(b) AI-agent / contributor's view.** The repo is unusually **agent-legible**:
explicit roles, an evidence rule, a doc registry (`docs/docmap.toml`), an agent
bus ledger, and a northstar spec. A new agent can orient quickly. Friction: the
northstar/architecture canon is still on branches, and the WP/evidence state lives
in `docs/agent-bus/` on a side branch rather than `main`.

## 4. Reach / visibility

| Metric | Value (2026-06-14) |
|---|---|
| Stars | 1 |
| Forks | 0 |
| Open issues | 30 |
| Repo age | ~3 weeks (created 2026-05-25) |
| Traffic (views/clones) | collected by `reassess.yml` via API each cycle |

README is concise (~111 lines) and design-rule focused; there is no landing
page, no screenshots, no rendered evidence, and no "try it" path. Discoverability
is effectively zero (no announcement, single star) — expected for the phase, but a
gap once there is something to show.

## 5. Verdict & course-corrections

**Verdict: ON-TRACK for an R&D/evidence-building project, but PRE-VISIBILITY.**
The discipline (evidence rule, northstar, harness) is the right foundation and is
being followed. The risk is not drift of *direction* but of *staying invisible*:
deep infra with no outward-facing proof.

**Course-corrections (1–3):**
1. **Close the first full evidence story.** Land the ENERGY gate (PR #75 battery
   run) so both SPEC §6 gates (latency + energy) are green — the first complete,
   honest proof point.
2. **Get the canon onto `main`.** Merge or explicitly close research PRs
   #50/#51/#52 so the objective function and architecture are visible in the trunk,
   not stranded on branches.
3. **Make the proof visible.** Turn the latency (and soon energy) evidence into a
   README chart / short "what this proves" section, so a human visitor sees the
   result, not just the rules.

---

_How this doc is maintained: each cycle, `reassess.yml` opens/updates a
"Strategic Reassessment" issue with auto-collected metrics + the checklist; a
human or agent fills the judgment sections, updates this file, drops a dated
snapshot in `docs/strategy/reassessments/`, and posts the verdict as an issue
comment. Snapshots are append-only data and are not individually registered in
`docs/docmap.toml`._
