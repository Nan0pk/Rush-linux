# Architecture Decision Records

This directory holds the project's Architecture Decision Records (ADRs). An ADR
captures one significant, hard-to-reverse decision: its context, the decision,
and its consequences. ADRs are the mechanism by which architectural
disagreements are resolved (see `docs/project-sustainability.md`, item C1).

## Lifecycle and states

Every ADR declares a `Status:` line as one of:

- **proposed** — written but not yet binding. May be revised freely. Code and
  config must not assume a proposed ADR is in force.
- **accepted** — ratified and binding. Changes that contradict an accepted ADR
  are out of policy and should be rejected in review.
- **superseded** — replaced by a later ADR. Must name the superseding ADR.
- **rejected** — considered and declined. Kept for the historical record.

## Who ratifies

The **maintainer of record — GitHub [@Nan0pk](https://github.com/Nan0pk)** — is
the ratifying authority. Only a human maintainer may move an ADR from
`proposed` to `accepted`.

This matters because agents (and automated contributors) author ADRs. To stop a
future agent from silently treating its own `proposed` decision as binding, the
following rule is **enforced by `tools/validate-repo.ps1` (test tier T0)**:

> Any ADR numbered **0008 or higher** that is marked `Status: accepted` MUST
> also contain a `Ratified-by:` line recording the human who ratified it and the
> date. An accepted agent-era ADR with no `Ratified-by:` line fails validation.

ADRs 0001–0007 predate this rule and are grandfathered (they were authored and
accepted before the agent-ratification policy existed).

### Ratifying a proposed ADR

A maintainer ratifies by editing the ADR to:

1. change `Status: proposed` to `Status: accepted`, and
2. add a line:

   ```text
   Ratified-by: <name or GitHub handle>, <YYYY-MM-DD>
   ```

3. remove the "needs human ratification" callout near the top.

## Current status

Read each ADR's `Status:` line; the set changes as the project develops. ADR
0025 is the accepted check model; ADR 0027 updates its merge authority and
independent review requirement. Proposed ADRs remain
non-binding until the maintainer adds an explicit `Ratified-by:` line.

## Adding a new ADR

Copy the structure of an existing ADR, take the next free number, and open it as
`proposed`. Reference it from the docs and code it affects.
