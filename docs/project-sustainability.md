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

Current gap (partially resolved): `CONTRIBUTING.md` has been rewritten with a
welcome-first structure. Issue/PR templates, `good first issue` labels, GitHub
Discussions, an AUTHORS file, and an onboarding tutorial have been added.

Remaining:

- **Community space:** stand up one real-time channel (Matrix or Discord) and
  link it from the README before the project acquires public presence.
- **Dev container:** publish a canonical dev container image (toolchain + Rust +
  build deps) so contributors can click "Open in Codespaces" and have a working
  build environment.

Completed:

- **Code of Conduct:** adopted Contributor Covenant v2.1 (`CODE_OF_CONDUCT.md`).
- **CONTRIBUTING.md rewritten:** welcome-first structure with ways to contribute,
  quick start guide, first-contribution walkthrough, review process, and
  communication channels.
- **Issue/PR templates:** `.github/ISSUE_TEMPLATE/` (bug report, feature request,
  question) and `.github/pull_request_template.md` with testing and docs
  checklists.
- **Good first issues:** label set created (`good first issue`, `type:bug`,
  `type:design`, `type:enhancement`, `type:question`, `area:optid`,
  `area:packaging`, `area:boot`, `area:docs`, `needs-triage`); starter issues
  seeded.
- **PR review policy:** documented in CONTRIBUTING.md (7-day initial review target,
  named criteria).
- **Contributor recognition:** `AUTHORS` file created; contributors acknowledged
  in release notes.
- **Onboarding tutorial:** `docs/contributing/first-pr.md` — step-by-step guide.
- **Governance:** ADR ratification documented (`docs/decisions/README.md`);
  agent-authored decisions recorded per `docs/agent-decisions/`.

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
