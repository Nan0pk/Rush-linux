# ADR 0029: Impact-based freshness for completed-package verification

Status: accepted
Ratified-by: Nan0pk, 2026-09-13 (explicit owner approval in the autonomous Rush work session)

Date: 2026-09-13
Tags: workflow, optid, verification, evidence, review

## Context

Runtime power-management PR #471 exposed that the existing proof-path freshness rule requires full cold re-verification of every completed package whenever any declared proof file changes, even when the package's proven behavior is unaffected.

The old rule correctly prevented a stale receipt from silently surviving arbitrary later edits, but it equated file modification with behavioral invalidation. Shared Optid files are proof paths for several completed packages, so ordinary construction can otherwise force repeated full certification of many historical packages even when their established contracts are unchanged.

## Decision

A completed package does not lose its completed status merely because a later change edits a file named in its `runtime_entrypoints`, `integration_tests`, or `completion_evidence`.

A later change that touches one or more declared proof paths of a completed package requires an **independent impact review** before merge. The reviewer must inspect the actual changed behavior and the package's original acceptance/proof scope and record one of these outcomes for each affected completed package:

1. **proof preserved** — the changed lines do not materially invalidate the behavior or evidence that established completion. The existing cold-verification receipt remains valid historical proof and the package stays `completed`.
2. **re-verification required** — the change can materially affect one or more acceptance claims or makes the old proof insufficient. The package may not continue to rely on the old receipt; fresh independent cold verification is required before its completion claim can be relied on for dependency/release truth.
3. **inconclusive** — impact cannot be established from available evidence. Treat this the same as re-verification required until the uncertainty is resolved.

The impact review is separate from builder self-checking. It may be performed as part of the mandatory independent merge review when that reviewer actually examines every affected completed package's acceptance/proof scope. The PR record must name the affected packages, touched proof paths, reasoning, checks/evidence inspected, exact head/base SHAs, and outcome. A generic `ready` verdict without that analysis does not satisfy this decision.

If the change or base moves, the impact judgment is stale to the same extent as the merge review and must be refreshed for the new head/base.

## Guardrails

- Builders cannot declare their own impact review sufficient.
- CI success, a bot label, silence, an older review, or the mere fact that existing tests still pass is not an impact review.
- An impact review cannot widen the original completion claim or create new hardware/performance evidence.
- Security-boundary changes, difficult-to-recover hardware writes, release claims, and other cases that independently require cold verification retain those requirements.
- If a package's acceptance behavior changed materially, use fresh cold verification; do not rationalize the old receipt as preserved.
- This decision does not permit a builder to mark a package `completed`, bypass dependencies, or change package state outside the existing package-completion contract.

## Tooling consequence

`tools/validate-optid-packages.py` continues to fail closed when the receipt commit is unavailable or structurally invalid. It no longer treats every later edit to a declared proof path as automatic proof invalidation. The repository's independent-review protocol is responsible for the semantic impact judgment because file-level diff detection cannot determine whether a behavioral proof remains valid.

The validator surfaces a completed package's proof-path edit as review context only when the edit is part of the currently proposed `--base` to `HEAD` change. An edit that already landed on `base` — because an earlier, unrelated change introduced and was reviewed for it — does not keep resurfacing on every later, unrelated change: once merged, it is part of that base for everything that follows. This is what makes a `proof preserved` outcome durable: nothing about the validator's signal would otherwise ever stop repeating for the life of the receipt.

## Application to PR #471

This decision removes the automatic requirement to cold-verify seven historical packages solely because PR #471 edits their shared proof files. It does **not** authorize PR #471 to merge by itself. PR #471 still needs an independent review on its current exact head/base that explicitly evaluates the impact on each affected completed package, followed by required exact-head CI and the normal delegated merge checks.
