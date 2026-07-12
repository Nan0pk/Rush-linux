# ADR 0022: rush-ai-harness — optional, non-executing AI consultation

Status: proposed

> Marked **proposed**; needs human ratification. This ADR scopes the
> LiveDev AI harness crate before any code lands. Do not assume this
> crate exists until this ADR is `accepted` with a `Ratified-by:` line
> and the transition plan in `docs/plans/livedev-transition-plan.md` has
> executed the corresponding phase.

Date: 2026-07-13
Authors: Z.ai (inconsistency-audit reconciliation)
Tags: architecture, livedev, automation, ai, safety

## Context

`docs/plans/livedev-transition-plan.md` Phase 6 introduces a new crate,
`rush_ai`, that wraps an external AI provider behind a typed Rust
interface so that the LiveDev track can consult a model without leaking
shell strings, free-form prompts, or execution authority into the AI
call path. The plan describes the harness's scope, its budget cap, its
mock contract, and the non-execution rule, but no ADR has ratified
that scope. This ADR fills the gap.

## Decision

Introduce one workspace crate:

- `crates/rush_ai` — a typed AI consultation harness. Wraps a single
  external provider behind a Rust trait; ships a mock implementation
  for tests.

### Non-execution rule

The harness does **not** execute commands, does **not** modify files,
does **not** modify release truth, and does **not** run during
benchmark sessions (per the LiveDev policy §4). It is a pure
question-answer surface: in goes a typed prompt, out comes a typed
response.

### Budget cap

Every call carries a token budget. A response that would exceed the
budget is truncated or rejected. The cap is configurable per-call but
has a hard ceiling set in code so a misbehaving provider cannot drain
the operator's account.

### Mock contract

The mock implementation is the contract: any test in the workspace
that wants to exercise the AI call path uses the mock, never a live
provider. CI never makes a live AI call.

## Consequences

- AI consultation is layered *above* the planner/runner: an AI caller
  may propose a different intent, but the planner's elaboration
  remains deterministic (ADR 0020) and the runner's execution remains
  typed (ADR 0021).
- The harness is the *only* LiveDev component permitted to call an
  external AI provider. Other components that want AI consultation
  must go through the harness.
- The harness crate lives in the workspace as a member; CI compiles
  it on every PR.

## Non-goals

- The harness does **not** modify the planner (Phase 4). The planner
  remains deterministic. AI consultation is an **optional caller** of
  the harness, not a planner integration.
- The harness does **not** merge PRs, mark evidence verified, or
  modify release truth (policy §§7, 8).
- The harness does **not** run during benchmark sessions (policy §4).

## Deliverables (from the transition plan)

- `crates/rush_ai/{Cargo.toml,src/lib.rs,src/main.rs,tests/,tests/fixtures/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_ai`.

## References

- `docs/plans/livedev-transition-plan.md` — Phase 6 (where this ADR is cited).
- ADR 0018 — Rush LiveDev Architecture Contract (parent).
- `docs/ai-interface-policy.md` — the policy that constrains this harness.
