# Documentation Policy

Rush Linux treats documentation as part of the implementation. A change is not
complete until the docs explain what changed, why it changed, how to verify it,
and what future maintainers must not break.

## Doc Management System

The project uses `docs/docmap.toml` as an **active documentation map**. It
tracks maintained user, architecture, workflow, and release documents—not
every research paper, archived audit, historical plan, or machine-readable
fixture.

Individual ADRs are indexed by `docs/decisions/README.md`; research papers are
indexed by `docs/research/README.md`. Archives and `docs/inbox/` are deliberately
outside the active map.

The automated sync validator (`tools/validate-doc-sync.py`) runs in CI and
catches drift, broken links, version mismatches, stale patterns, and
contradictions between docs.

See [docs/contributing/keeping-docs-synced.md](contributing/keeping-docs-synced.md)
for the full guide on updating docs and using the docmap.

## Required When the Documented Truth Changes

Update documentation when a change alters:

- a user command or public behavior;
- a safety rule or privileged action;
- an accepted decision or project direction;
- a release gate or evidence format;
- an interface another contributor must understand.

Do not require unrelated README, roadmap, status, or handoff edits merely
because code changed. That creates noise and teaches agents to make meaningless
documentation changes to satisfy a gate.

## Required Docs By Change Type

- `optid` or `optctl` behavior: update the document that describes the changed
  behavior; update implementation status only when feature status changed.
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
- architectural direction: add or amend an ADR and update only the strategy or
  architecture documents whose direction actually changed.

## Minimum Commit Standard

Before committing:

- update docs in the same commit as code/config changes;
- run `bash tools/checks.sh`;
- include validation status in the final handoff;
- record follow-up only when real work remains.

## Forbidden

- "Code now, docs later."
- Behavior changes without status/roadmap updates.
- Silent safety changes to privileged services or sysfs writes.
- Release-gate changes without updating machine-readable release manifests.
- Provider-specific instructions that duplicate `AGENTS.md` or the canonical
  check commands.
- Historical reports at the repository root.

## Root Directory Hygiene

The project root must contain only files that tooling, GitHub, or strong
convention requires there. Everything else belongs in `docs/`.

**Permitted at root:** `README.md`, `LICENSE`, `CONTRIBUTING.md`,
`CODE_OF_CONDUCT.md`, `SECURITY.md`, `RELEASES.md`, `ROADMAP.md`,
`SUPPORT.md`, `AUTHORS`, `VERSION`, `AGENTS.md`, `CLAUDE.md`,
`OPTID-COMPLETION-PLAN.md`, `Cargo.toml`,
`Cargo.lock`, and dotfiles (`.gitignore`, `.gitattributes`, etc.).

**Must live under `docs/`:** status reports, strategy documents,
reanalysis reports, work plans, research notes, implementation status,
project brief, agent handoff context, and any other prose not required
by GitHub or external tooling at the root.

Completed audits belong in `docs/audit-archive/`. A short compatibility pointer
may remain only when source comments still cite an old root path. Git history
preserves deleted superseded handoffs; they do not remain active merely for
historical context.

**Inbox for unsorted drafts:** drop transient notes and in-progress
reports in `docs/inbox/`. Files there are not registered in
`docs/docmap.toml` and are not validated by CI. Sort or promote them
before merging to main.
