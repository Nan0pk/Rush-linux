# ADR 0027: Delegate reviewed repository merges

Status: accepted
Ratified-by: Nan0pk, 2026-09-05 (explicit owner direction in the governance session)

Date: 2026-09-05
Tags: workflow, agents, review, integration

## Context and authority

The owner explicitly said they do not want to approve every PR or make AI work
wait for their availability. They want accuracy and completeness reviewed by
another agent, with AI continuing while the owner gives direction. This records
that supplied authority; it does not claim the owner personally reviewed every
implementation detail in this change.

The human-only merge rule duplicates across instructions and submission
messages. It conflates code integration with release/hardware authority. The
live main ruleset requires zero approving GitHub reviews, required CI and
resolved threads. Its four legacy CI aliases still exist and must not be
removed independently of the live configuration.

## Decision

Delegate repository integration within approved direction to a coordinating
agent after a separate agent reviews accuracy and completeness. The coordinator
resolves findings, verifies the current PR, review and CI, and merges through
GitHub's protected interface with the expected head SHA. It continues eligible
work without a new owner confirmation.

Use [the agent protocol](../agent-protocol.md) as the single detailed procedure.
A short review is enough for a small change. Reuse qualifying high-risk cold
verification; preserve package completion receipts and actual hardware evidence.
Do not create another certification bureaucracy, merge bot or identity fiction.

New strategic direction, release/milestone declarations, trusted hardware
promotion, production signing and destructive physical actions retain their
existing authority. Existing authorization remains valid. Technical choices
within approved direction are engineering work, not automatic prompts.

Collectors and unattended repository scripts remain unable to self-merge.
An active coordinator uses existing agent and GitHub tools; this decision does
not create an always-on worker or change credentials or repository settings.

## Supersession

Supersedes ADR 0025's human-only merge clauses and its exemption of ordinary
changes from independent **merge review**. Ordinary builder checks remain the
builder's responsibility; development and opening PRs do not wait for review.
All other risk/evidence protections in ADR 0025 remain in force.

Supersedes conflicting human-only integration wording in older work plans and
LiveDev design documents for the coordinating role only. Collector/submission
restrictions remain intact. It does not ratify unrelated proposed architecture
or the separate OS-goal amendment in PR #456.

## Consequences

Assess owner interventions, ready-to-merge waiting time, reviewer findings,
post-merge regressions and dependency stalls from actual PR history. Fewer
prompts alone do not prove better quality. If review becomes the bottleneck,
focus it on affected behavior and parallelize independent work; retain the
independent judgment.

An independent reviewer checks this policy change before integration. GitHub
enforces its branch protection. Separate agents using one GitHub identity
provide auditable process evidence, not independently authenticated identities.
