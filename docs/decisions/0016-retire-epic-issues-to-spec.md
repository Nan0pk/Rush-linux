# ADR 0016: Migrating Epic and Work Package Tracking out of GitHub Issues

Status: accepted
Ratified-by: Nan0pk, 2026-06-17

> This ADR establishes a strict semantic boundary for GitHub Issues, migrating multi-year Epics and Work Package (WP) specifications out of open issues and consolidating them natively inside our documentation canon and project boards.

## Context

During foundational development, long-term Work Package specifications (such as `WP-N1` through `WP-N9` and `WP-B1`) and overarching organizational epics (`Track A` through `Track D`) were registered as standard open GitHub Issues (Issues #37–#40 and #54–#63). Additionally, weekly living Strategic Reassessment reports were filed as open issues (e.g., Issue #83).

While this kept all concepts in one platform during initial AI-agent bootstrapping, it has created substantial semantic friction. When human developers, security auditors, or AI assistants visit the repository, seeing a high count of open issues creates the false impression of technical debt or outstanding codebase defects. Long-term roadmaps are not software bugs; keeping them open as issues creates noise and makes legitimate, actionable onboarding tasks (like `good first issue` #3) harder to discover.

## Decision

1. **Strict Semantic Rule for Issues**: Reserve GitHub Issues exclusively for **reproducible software defects**, operational CI broken gates, and **self-contained, actionable onboarding tasks** (e.g., `good first issue`).
2. **Consolidate Epics and Work Packages into Spec Canon**: Multi-year specifications (`WP-N*`, `WP-B*`) and Track epics (`Tracks A–D`) belong natively in our Git-tracked markdown architecture (`docs/SPEC-northstar.md`, `ROADMAP.md`, `docs/plans/agent-work-plan-v1.md`) and GitHub Projects/Discussions. They must not exist as open GitHub Issues.
3. **Living Ledgers Natively in Docs**: Strategic Reassessments belong in `docs/strategy/reassessments/` and `COMPASS.md`, superseding tracker issues.
4. **Mass Migration and Closure**: Formally close legacy Epics (Issues #37–#40), Work Package tracker RFCs (Issues #54–#63), and reassessment ledgers (Issue #83) on GitHub, leaving forwarding pointers to the canonical markdown specifications.

## Consequences

- The repository will present a radically authentic, low open-issue count reflecting only genuine, actionable tasks.
- Visiting contributors can instantly identify valid onboarding surfaces without filtering out specification noise.
- Specification evolution is governed through Pull Requests against `docs/SPEC-northstar.md` rather than un-auditable issue comments.
