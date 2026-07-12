# ADR 0020: rush-planner — deterministic plan elaboration

Status: proposed

> Marked **proposed**; needs human ratification. This ADR scopes the
> LiveDev planner crate before any code lands. Do not assume this crate
> exists until this ADR is `accepted` with a `Ratified-by:` line and the
> transition plan in `docs/plans/livedev-transition-plan.md` has executed
> the corresponding phase.

Date: 2026-07-13
Authors: Z.ai (inconsistency-audit reconciliation)
Tags: architecture, livedev, automation, determinism

## Context

`docs/plans/livedev-transition-plan.md` Phase 4 introduces a new crate,
`rush_planner`, that elaborates a high-level operator intent ("benchmark
mixed-load-001 on host laptop-a on battery") into a concrete sequence of
`CommandSpec` invocations that `rush-exec` (ADR 0019) can run. The plan
describes the planner's scope and its determinism rule, but no ADR has
ratified that scope. This ADR fills the gap.

## Decision

Introduce one workspace crate:

- `crates/rush_planner` — a deterministic plan elaborator. Given an
  operator intent (typed) and a capture context, produces a fixed
  sequence of `CommandSpec` invocations.

### Determinism rule

For the same inputs (intent + context), the planner MUST produce the
same plan. There is no clock, no random source, no AI consultation
inside the planner. This is the property that lets a re-run produce a
reproducible capture.

### Non-goals

- The planner does **not** execute plans (Phase 5 / ADR 0021 does).
- The planner does **not** call AI (Phase 6 adds optional AI
  consultation as a *caller* of the planner, not as a planner
  integration).
- The planner does **not** modify `release/milestones.toml` or any
  release-truth file.

## Consequences

- A re-run with the same intent + context reproduces the same plan
  byte-for-byte.
- AI consultation is layered *above* the planner: an AI caller may
  propose a different intent, but the planner's elaboration is
  deterministic.
- The planner crate lives in the workspace as a member; CI compiles it
  on every PR.

## Deliverables (from the transition plan)

- `crates/rush_planner/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_planner`.

## References

- `docs/plans/livedev-transition-plan.md` — Phase 4 (where this ADR is cited).
- ADR 0018 — Rush LiveDev Architecture Contract (parent).
- ADR 0019 — rush-exec / rush-capture (executor that consumes plans).
