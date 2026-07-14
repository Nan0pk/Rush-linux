# ADR 0021: rush-runner — LiveDev orchestration over testOS

Status: proposed

> Marked **proposed**; needs human ratification. This ADR scopes the
> LiveDev runner crate before any code lands. Do not assume this crate
> exists until this ADR is `accepted` with a `Ratified-by:` line and the
> transition plan in `docs/plans/livedev-transition-plan.md` has executed
> the corresponding phase.

Date: 2026-07-13
Authors: Z.ai (inconsistency-audit reconciliation)
Tags: architecture, livedev, automation, testos

## Context

`docs/plans/livedev-transition-plan.md` Phase 5 introduces a new crate,
`rush_runner`, that orchestrates a LiveDev capture session end-to-end:
it consumes a plan from `rush-planner` (ADR 0020), invokes `rush-exec`
(ADR 0019) for each step, records capture state through
`rush_capture`, and writes the result bundle. The plan describes the
runner's scope and its prompt contract, but no ADR has ratified that
scope. This ADR fills the gap.

## Decision

Introduce one workspace crate:

- `crates/rush_runner` — a LiveDev session orchestrator. Consumes a
  plan, executes it via `rush-exec`, captures state via
  `rush_capture`, writes a result bundle to disk.

### Prompt contract

The runner accepts a single typed input (a plan from `rush-planner`)
and produces a single typed output (a result bundle path + a verdict).
No shell strings, no free-form operator input mid-session.

### Relationship to testOS

testOS is **not** modified, deprecated, or renamed by this crate. The
runner is the long-term successor path for the LiveDev workflows that
testOS's single-shot appliance model cannot serve; testOS remains the
"try it on real hardware" target.

## Consequences

- The runner is the only LiveDev component that owns a session's
  lifecycle. The planner does not execute; the capture primitive does
  not orchestrate.
- A re-run with the same plan reproduces the same capture (modulo
  hardware nondeterminism, which `rush_capture` records).
- The runner crate lives in the workspace as a member; CI compiles it
  on every PR.

## Non-goals

- The runner does **not** call AI (Phase 6 / ADR 0022 adds optional AI
  consultation as a caller of the runner, not as a runner integration).
- The runner does **not** boot a LiveDev image (Phase 7 of the
  transition plan).
- The runner does **not** open PRs (Phase 8 of the transition plan).

## Deliverables (from the transition plan)

- `crates/rush_runner/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_runner`.
- `docs/plans/testos-transition.md` (new).

## References

- `docs/plans/livedev-transition-plan.md` — Phase 5 (where this ADR is cited).
- ADR 0018 — Rush LiveDev Architecture Contract (parent).
- ADR 0019 — rush-exec / rush-capture (primitives the runner uses).
- ADR 0020 — rush-planner (source of the plans the runner executes).
