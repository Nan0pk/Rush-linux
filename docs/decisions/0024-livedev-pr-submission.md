# ADR 0024: Rush LiveDev PR Submission

Status: superseded

> Superseded by ADR 0025. The safe PR-submission behavior remains, but its
> separate LiveDev-only CI workflow was replaced by one change-aware workflow.

Date: 2026-07-04
Authors: Z.ai (pr-ci phase)
Tags: architecture, livedev, pr, ci, evidence

## Context

The LiveDev track has produced a suite of tools (`rush-exec`,
`rush-capture`, `rush-autopilot`, `rush-agent`) that run benchmarks,
capture evidence, and attempt AI-assisted repair. Phase 8 adds the PR
submission surface: the ability to open PRs with evidence or code
attached, so the Verifier and Human can review and merge.

The old testOS collectors created PRs and merged them automatically. That unsafe
behavior has now been removed from testOS as well as LiveDev. Both open PRs for
maintainer review and never merge them.

## Decision

### 1. PR submission surface

`tools/rush_pr_lib.py` + `rush-autopilot submit-evidence` /
`submit-failing-evidence` / `submit-code-pr` subcommands. Implemented
as Python (matching the rest of the LiveDev tool suite).

### 2. Submission behavior

1. Validate evidence locally (`validate-hwtest-evidence.py`).
2. Run privacy/secret scan (`rush_capture_lib.redact`).
3. Create deterministic branch name (`evidence/livedev-<date>-<hash>`).
4. Copy evidence into `release/evidence/livedev-<branch>/`.
5. Commit with deterministic message.
6. Push branch.
7. Open PR via `gh` CLI or GitHub API.
8. Redact tokens from all logs.
9. Never merge.
10. Never mark milestone verified.

### 3. Dry-run mode

`--dry-run` shows: files to add, branch name, commit message, PR title/body,
validation status — without pushing or creating a PR.

### 4. CI integration

The change-aware workflow established by ADR 0025 runs the relevant evidence
checks when a PR touches evidence, schemas, or LiveDev tools. It validates:
- Schema validation
- Semantic evidence validator
- Privacy/secret scan
- Provenance checks
- Event-chain validation
- AI summaries not treated as evidence
- Release truth not changed by evidence PR
- No self-merge commands

### 5. Milestone-close separation

Evidence PRs contain raw evidence only. Milestone-close PRs (flipping
`verified = true` in `release/milestones.toml`) are separate and require
explicit maintainer approval.

### 6. Forbidden paths

The PR submission blocks modifications to:
- `VERSION`, `Cargo.toml`, `RELEASES.md`
- `release/milestones.toml`, `release/test-tiers.toml`
- `.github/workflows/ci.yml`
- `docs/decisions/` (ADR Status/Ratified-by lines)
- `mkosi/mkosi.extra/etc/os-release`
- Existing evidence subdirs (`v0.3.0-alpha.1/`, `v0.4.0-alpha.1/`,
  `v0.5.0-beta.1/`, `dragnet/`, `core-tests/`, `host-bench/`)

### 7. No self-merge

There is no merge command in the rush PR tools. The `has_merge_command()`
function always returns `False`. CI checks for merge-related strings and
fails if found.

## Consequences

- LiveDev can prepare and open PRs safely.
- CI owns validation — the PR submission does not self-verify.
- The Human reviews and merges.
- Release truth is never modified by evidence PRs.
- testOS follows the same no-merge rule.

## Acceptance criteria

- [ ] `tools/rush_pr_lib.py` exists with submission engine.
- [ ] `rush-autopilot submit-evidence --dry-run` works.
- [ ] `rush-autopilot submit-failing-evidence --dry-run` works.
- [ ] `rush-autopilot submit-code-pr --dry-run` works.
- [ ] `.github/workflows/ci.yml` invokes the central relevant-check runner.
- [ ] `docs/templates/livedev-pr.md` exists.
- [ ] No merge command exists in the rush tools.
- [ ] Release truth files are blocked.
- [ ] Tests pass.

## References

- ADR 0018 (LiveDev architecture contract) — §6.2 no self-merge.
- `docs/ai-interface-policy.md` — §§7-8 no merge, no release truth.
- `docs/agent-protocol.md` — authority matrix (Builder = open PRs, not merge).
- `docs/plans/livedev-transition-plan.md` Phase 8.
- `testos/collect-results.sh` — existing PR creation pattern (reference only).
