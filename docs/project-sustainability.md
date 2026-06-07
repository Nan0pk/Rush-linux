# Project Sustainability

Rush Linux is architecturally rigorous at the OS layer but has had almost no
infrastructure for the human/organisational layer. A distro cannot reach v1.0 as
a one-entity project: the T3 (hardware), T4 (benchmark), and T5 (security) gates
all require multiple people and real hardware. These items have the longest lead
time and the least urgency until they are suddenly the only thing blocking a
release — so they are started now, at v0.3/v0.4, deliberately.

This document is the plan for the people/infrastructure items the expert review
flagged (C1, C2, C8). It is a living plan, not a finished policy.

## C1 — Contributor model, governance, community

Current gap: `CONTRIBUTING.md` describes rules but not how to get involved;
there is no triage process, no review policy, no governance, no community space,
no code of conduct.

Plan:

- **Code of Conduct:** adopt the Contributor Covenant (see `CODE_OF_CONDUCT.md`).
- **Governance (lightweight, now):** document who holds merge rights and how
  architectural disagreements are resolved (ADRs are the mechanism; a named
  maintainer ratifies). Record agent-authored decisions per
  `docs/agent-decisions/`.
- **Issue triage:** define labels (`type:bug`, `type:design`, `good-first-issue`,
  `area:optid`, `area:packaging`, `area:boot`, `area:docs`) and a triage cadence.
- **PR review policy:** state the review bar (builds, tests, clippy clean, docs
  updated in the same change) and who reviews each area.
- **Community space:** stand up one real channel (Matrix or Discord) and link it
  from the README before the project acquires public presence.

## C2 — Canonical development environment is Linux

Current gap: docs framed Windows/WSL2 as the primary dev environment and
PowerShell as primary validation, even though the build artifacts require Linux
and CI is Linux-only.

Resolution (in progress):

- Linux (native or container) is declared canonical in `README.md`,
  `CONTRIBUTING.md`, `docs/validation.md`, and `AI_CONTINUATION.md`.
- `tools/validate-repo.ps1` is reframed as a cross-platform policy check (runs
  under `pwsh` on Linux in CI), not a Windows-only crutch.
- Follow-up: publish a canonical dev container image (toolchain + Rust + build
  deps) so "it works in CI" is backed by a reproducible local environment, and
  port `validate-repo.ps1`'s checks to a shell/Python equivalent so PowerShell
  is optional rather than load-bearing.

## C8 — Hardware test lab (required for beta, T3)

Current gap: T3 is a beta requirement but there is no plan to acquire or
maintain hardware. T3 needs an Intel laptop, an AMD laptop, a desktop with a
discrete GPU, a low-RAM system, an NVMe workstation, and a headless server.
Lab setup (sourcing, remote access, CI integration, suspend/resume and GPU test
scripts) takes months — so planning starts now, not at v0.5.

Plan:

- **Inventory & ownership:** name an owner per machine class; track each
  machine's specs, location, and access in a lab inventory.
- **Access & CI:** self-hosted GitHub Actions runners (or equivalent) on the lab
  machines, gated so untrusted PRs cannot run on them.
- **Result capture:** standard result schema (see ADR 0011) stored as build
  artifacts; results are per machine class, never aggregated.
- **Resilience:** define what happens when hardware dies or is unavailable for a
  release (which classes are release-blocking vs best-effort).
- **Timeline:** begin sourcing and runner setup during v0.4 so T3 is not the
  v0.5 critical-path blocker.

## Cross-reference

- Benchmark methodology and baselines: ADR 0011.
- Security review process and disclosure: ADR 0009 and `SECURITY.md`.
- Agent decision audit trail: `docs/agent-decisions/`.
