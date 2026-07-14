# ADR 0019: rush-exec and rush-capture — execution and capture primitives

Status: proposed

> Marked **proposed**; needs human ratification. This ADR scopes two new
> workspace crates before any code lands. Do not assume these crates exist
> until this ADR is `accepted` with a `Ratified-by:` line and the
> transition plan in `docs/plans/livedev-transition-plan.md` has executed
> the corresponding phase.

Date: 2026-07-13
Authors: Z.ai (inconsistency-audit reconciliation)
Tags: architecture, livedev, automation, primitives

## Context

`docs/plans/livedev-transition-plan.md` Phase 3 introduces two new crates,
`rush-exec` and `rush-capture`, as the execution and capture primitives
that the LiveDev track (and future tracks) build on. The plan describes
their scope, their non-goals, and their safety posture, but until now no
ADR has ratified that scope. This ADR fills the gap so the plan's
citations resolve.

## Decision

Introduce two workspace crates:

- `crates/rush-exec` — a typed command-execution primitive. Accepts a
  `CommandSpec` (a typed argv array), **not** a shell string. Same
  discipline as `crates/optid/src/io_util.rs::guarded_write`: no string
  interpolation into a shell. Captures stdout/stderr/exit-code/resource
  usage.
- `crates/rush_capture` — a capture-session manager. Owns the lifecycle
  of a capture session (start, stream samples, finalize, write manifest),
  but delegates execution to `rush-exec` and measurement to existing
  probes.

### Non-goals

- They are **not** a shell. No `sh -c "..."`.
- They are **not** a workflow engine. No plan interpretation, no
  conditional logic.
- They do **not** call AI. They are general primitives; LiveDev is their
  first consumer, not their only consumer.
- They do **not** modify release truth (`release/milestones.toml`,
  evidence directories).

## Consequences

- The LiveDev runner (ADR 0021) will be a thin orchestration layer over
  `rush-exec` + `rush_capture` + the existing `rushbench` probes.
- The "no shell string" rule is enforced at the API boundary: a
  `CommandSpec` that tries to invoke `sh -c "..."` is rejected at
  construction, not at runtime.
- Both crates live in the workspace as members; CI compiles them on
  every PR.

## Deliverables (from the transition plan)

- `crates/rush_capture/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entries for both crates.

## References

- `docs/plans/livedev-transition-plan.md` — Phase 3 (where this ADR is cited).
- `crates/optid/src/io_util.rs` — `guarded_write` pattern that
  `rush-exec` mirrors for command construction.
- ADR 0018 — Rush LiveDev Architecture Contract (parent).
