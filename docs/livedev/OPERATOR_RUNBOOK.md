# Rush LiveDev Operator Runbook

## Start here

After cloning the repo, run:

```sh
python3 tools/livedev-next
```

This prints the repo state, checks that all required LiveDev tools are present, and shows the next commands to run.

## Quick reference

| What | Command |
|---|---|
| Check repo state | `python3 tools/livedev-next` |
| Run mock tests | `python3 tools/livedev-next --mock` |
| Generate a plan | `python3 tools/livedev-next --plan` |
| Execute a plan (fake mode) | `python3 tools/livedev-next --run /tmp/rush-livedev-plan.json` |
| Submit evidence (dry-run) | `python3 tools/livedev-next --submit <RUN_DIR> --dry-run` |
| Submit evidence (real) | `python3 tools/livedev-next --submit <RUN_DIR>` |
| Full E2E dry run | `python3 tools/livedev-e2e-dry-run.py --all` |
| Validate evidence fixtures | `python3 tools/validate-hwtest-evidence.py --fixtures` |

## After clone

```sh
git checkout main
git pull --ff-only origin main
python3 tools/livedev-next
python3 tools/livedev-next --mock
```

The `--mock` command runs three end-to-end scenarios (success, failure without AI, failure with AI fix) plus the evidence fixture validation. All run in fake/mock mode — no hardware, no network, no real PRs.

## Generate a plan

```sh
python3 tools/livedev-next --plan
```

This calls `rush-autopilot plan --dry-run --output /tmp/rush-livedev-plan.json` and writes the plan to `/tmp/rush-livedev-plan.json`.

## Execute a plan

```sh
python3 tools/livedev-next --run /tmp/rush-livedev-plan.json
```

By default this runs in **fake mode** (no real hardware, no real commands). The run directory is printed in the output.

For real hardware runs, use the underlying tool directly:

```sh
python3 tools/rush-autopilot run --plan /tmp/rush-livedev-plan.json --run-dir /tmp/rush-run-001
```

## Validate evidence

After a run completes, validate the evidence bundle:

```sh
python3 tools/validate-hwtest-evidence.py --bundle <RUN_DIR>
```

Where `<RUN_DIR>` is the path printed by the run command.

## Submit evidence

Dry-run (shows what would be committed, no push, no PR):

```sh
python3 tools/livedev-next --submit <RUN_DIR> --dry-run
```

Real submission (creates a branch, commits evidence, pushes, opens a PR):

```sh
python3 tools/livedev-next --submit <RUN_DIR>
```

The tool will print `[TOKEN NEEDED]` if `GH_TOKEN` is not set. Export it and rerun:

```sh
export GH_TOKEN=github_pat_your_token_here
python3 tools/livedev-next --submit <RUN_DIR>
```

The token needs: Contents read/write, Pull requests read/write, Metadata read, Workflows read/write.

## Token timing

Only provide `GH_TOKEN` when the tool prints:

```
[TOKEN NEEDED]
```

Do not set the token in the environment before that point.

## Never automatic

- **No self-merge** — there is no merge command in any LiveDev tool.
- **No milestone verification** — `verified = true` in `release/milestones.toml` is set only by the human maintainer.
- **No release cut** — `VERSION`, `RELEASES.md`, tags are never modified by LiveDev tools.
- **No release-truth edit** — `VERSION`, `Cargo.toml` workspace version, `RELEASES.md`, `release/milestones.toml`, `release/test-tiers.toml`, `.github/workflows/ci.yml`, ADR `Status:` lines are all forbidden paths.
- **No fabricated hardware evidence** — all evidence must come from a real run directory produced by `rush-autopilot run`.

## Current state

- LiveDev foundation is infrastructure.
- It claims no real hardware evidence.
- Hardware evidence must be submitted in a separate evidence PR.
- Milestone/release truth must be changed only in a separate maintainer-approved PR.
- testOS is NOT replaced or deprecated. It remains the "Try it on real hardware" target.
