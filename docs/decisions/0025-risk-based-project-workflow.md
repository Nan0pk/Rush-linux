# ADR 0025: Risk-Based Project Workflow and Checks

Status: accepted
Ratified-by: Nan0pk, 2026-07-10

Date: 2026-07-10
Tags: workflow, research, ci, evidence, agents, safety

## Context

Rush accumulated separate workflows and gates for Rust, Clippy, evidence,
LiveDev, documentation, PowerShell, front-page generation, dependency review,
and recurring Dragnet checks. Several ran the same underlying check. Some ran on
changes they could not meaningfully judge. The local start command also blocked
documentation work when Cargo was not installed and hid the real error.

At the same time, broad wording such as "builders do not verify" allowed an
agent to treat ordinary self-testing as someone else's job. Proof rules that
were created after real false-certification incidents had become detached from
the exact claims they were meant to protect.

## Scoped amendment — 2026-09-05

[ADR 0027](0027-delegated-reviewed-merges.md) supersedes the human-only merge
clauses below and requires focused independent review before delegated merges.
The original decision remains historical context; its other protections remain.

## Decision

1. Use `docs/project-workflow.md` as the single path from intent through
   research, decision, implementation, proof, human merge, and observation.
2. Not every change uses every stage. The path is chosen by actual risk.
3. Accepted decisions outrank unfinished research. Research remains visible
   whether or not it has been validated.
4. Use one stable pull-request status, **PR Gate**, backed by
   `tools/checks.sh`. It selects tests from the files that changed and
   aggregates the Linux/repository, native Windows, and image lanes.
5. Keep external-link and new-advisory scans scheduled rather than blocking
   unrelated pull requests.
6. Builders run ordinary checks. Independent cold verification is reserved for
   hardware, security, boot, performance, milestone, and release claims.
7. Every blocker names its risk, historical or design root, missing proof, and
   safe alternatives.
8. Automation may open draft pull requests, but the **no self-merge** rule means
   it may never merge or enable auto-merge. The repository checks this rule
   itself.
9. Unverified hardware entries are candidates and cannot authorize automatic
   writes. A hardware-gate bypass is limited to a single experimental run.

## Consequences

- A documentation change no longer waits for a Rust toolchain.
- Rust changes still receive formatting, tests, and Clippy.
- Evidence checks run when evidence or release truth changes.
- Hardware proof blocks hardware promotion and claims, not read-only work or
  controlled experiments.
- The human sees one meaningful PR status instead of several overlapping ones.
- Release and publishing workflows remain separate because they create
  artifacts rather than judge an ordinary code change.

## Supersedes

- The separate CI portion of ADR 0024.
- The mandatory second-agent requirement for ordinary low-risk changes in the
  earlier Agent Work Protocol.
- Automatic merging by Dependabot and testOS collectors.
