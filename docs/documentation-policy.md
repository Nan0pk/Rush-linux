# Documentation Policy

Rush Linux treats documentation as part of the implementation. A change is not
complete until the docs explain what changed, why it changed, how to verify it,
and what future maintainers must not break.

## Doc Management System

The project uses `docs/docmap.toml` as a **doc registry** that tracks every
documentation file, its purpose, the code it covers, its dependencies on other
docs, and when it was last verified against the codebase.

The automated sync validator (`tools/validate-doc-sync.py`) runs in CI and
catches drift, broken links, version mismatches, stale patterns, and
contradictions between docs.

See [docs/contributing/keeping-docs-synced.md](contributing/keeping-docs-synced.md)
for the full guide on updating docs and using the docmap.

## Required For Every Change

Every non-trivial change must document:

- purpose and user/system impact;
- changed files, APIs, configs, services, recipes, or release gates;
- safety implications, especially for privileged actions;
- validation commands and results;
- follow-up work and known limitations.

## Required Docs By Change Type

- `optid` or `optctl` behavior: update `docs/adaptive-engine.md`,
  `docs/IMPLEMENTATION_STATUS.md`, and `docs/AI_CONTINUATION.md`.
- systemd service/sandboxing: update `docs/adaptive-engine.md`,
  `SECURITY.md`, and validation checks when relevant.
- kernel policy: update `docs/kernel-policy.md` and relevant ADRs.
- boot/update flow: update `docs/boot-and-updates.md`,
  `docs/release-checklist.md`, and release gates.
- packaging/build logic: update `docs/packaging-and-builds.md` and recipes.
- testing/benchmarks: update `docs/testing-and-benchmarks.md`,
  `benchmarks/manifest.toml`, and `release/test-tiers.toml`.
- version/release rules: update `VERSION`, `RELEASES.md`,
  `docs/versioning.md`, `docs/release-policy.md`, and
  `release/milestones.toml`.
- architectural direction: update `docs/PROJECT_BRIEF.md`, `docs/architecture.md`,
  and add or amend an ADR.

## Automated Enforcement

Documentation completeness is **enforced in CI**, not by manual checklists.

- **`validate-pr-docs.py`** — runs on every PR. Uses `docs/docmap.toml`'s
  `covers_code` to find which docs cover the changed code. If code changes
  lack corresponding doc updates (or a `last_verified` bump in docmap.toml),
  the PR is blocked.
- **`validate-doc-sync.py`** — runs on every push/PR. Catches drift, broken
  links, version mismatches, stale patterns, and contradictions.
- **Auto-labeling** — PR labels are applied automatically by file paths
  (area labels) and PR title prefix (type labels via conventional commits).
  No manual labeling needed.
- **Auto-changelog** — release-drafter categorizes merged PRs from labels.
  No manual listing needed.

## Minimum Commit Standard

Before committing:

- update docs in the same commit as code/config changes;
- run `powershell -ExecutionPolicy Bypass -File .\tools\validate-repo.ps1`;
- include validation status in the final handoff;
- keep `docs/AI_CONTINUATION.md` current when the next task changes.

## Forbidden

- "Code now, docs later."
- Behavior changes without status/roadmap updates.
- Silent safety changes to privileged services or sysfs writes.
- Release-gate changes without updating machine-readable release manifests.

## Root Directory Hygiene

The project root must contain only files that tooling, GitHub, or strong
convention requires there. Everything else belongs in `docs/`.

**Permitted at root:** `README.md`, `LICENSE`, `CONTRIBUTING.md`,
`CODE_OF_CONDUCT.md`, `SECURITY.md`, `RELEASES.md`, `ROADMAP.md`,
`AUTHORS`, `VERSION`, `AGENTS.md`, `CLAUDE.md`, `Cargo.toml`,
`Cargo.lock`, and dotfiles (`.gitignore`, `.gitattributes`, etc.).

**Must live under `docs/`:** status reports, strategy documents,
reanalysis reports, work plans, research notes, implementation status,
project brief, agent handoff context, and any other prose not required
by GitHub or external tooling at the root.

**Inbox for unsorted drafts:** drop transient notes and in-progress
reports in `docs/inbox/`. Files there are not registered in
`docs/docmap.toml` and are not validated by CI. Sort or promote them
before merging to main.

