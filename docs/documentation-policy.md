# Documentation Policy

Rush Linux treats documentation as part of the implementation. A change is not
complete until the docs explain what changed, why it changed, how to verify it,
and what future maintainers must not break.

## Required For Every Change

Every non-trivial change must document:

- purpose and user/system impact;
- changed files, APIs, configs, services, recipes, or release gates;
- safety implications, especially for privileged actions;
- validation commands and results;
- follow-up work and known limitations.

## Required Docs By Change Type

- `optid` or `optctl` behavior: update `docs/adaptive-engine.md`,
  `IMPLEMENTATION_STATUS.md`, and `AI_CONTINUATION.md`.
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
- architectural direction: update `PROJECT_BRIEF.md`, `docs/architecture.md`,
  and add or amend an ADR.

## Minimum Commit Standard

Before committing:

- update docs in the same commit as code/config changes;
- run `powershell -ExecutionPolicy Bypass -File .\tools\validate-repo.ps1`;
- include validation status in the final handoff;
- keep `AI_CONTINUATION.md` current when the next task changes.

## Forbidden

- "Code now, docs later."
- Behavior changes without status/roadmap updates.
- Silent safety changes to privileged services or sysfs writes.
- Release-gate changes without updating machine-readable release manifests.

