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

## What exists today

- **Planner** (`rush-autopilot plan`) — reads repo state + hardware, generates a typed plan. Wired.
- **Runner** (`rush-autopilot run`) — executes plans through `rush-exec`, captures sessions through `rush-capture`. Wired (fake mode works; real hardware requires actual hardware).
- **Evidence validator** (`validate-hwtest-evidence.py`) — 14 semantic checks on evidence bundles. Wired.
- **AI harness** (`rush-agent`) — mock provider for dev-if-fail repair. Wired (mock only; real providers not ratified).
- **PR submission** (`rush-autopilot submit-evidence`) — dry-run and real submission. Wired.
- **LiveDev image** (`mkosi/mkosi.profiles/livedev/`) — mkosi profile skeleton. Created but not yet built on real hardware.
- **E2E dry run** (`livedev-e2e-dry-run.py`) — three scenarios that exercise the full loop in fake/mock mode. Wired.

## What is not wired yet

- **Real AI providers** — only the mock provider is available. Real providers (OpenAI, Anthropic, etc.) require maintainer ratification via an ADR.
- **Real hardware evidence** — no physical hardware transcripts have been submitted. The v0.6.0-beta.1 milestone criteria remain `verified = false`.
- **LiveDev image boot** — the mkosi profile exists but has not been built or booted on real hardware.
- **Milestone close** — milestone-close PRs (flipping `verified = true`) are separate from evidence PRs and require explicit maintainer approval.

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

## testOS compatibility

testOS is NOT replaced or deprecated by LiveDev. testOS remains the "Try it on real hardware" target. LiveDev is a parallel track for continuous operation. The two coexist.
