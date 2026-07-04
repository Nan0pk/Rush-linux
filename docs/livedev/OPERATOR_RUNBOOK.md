# Rush LiveDev Operator Runbook

## Start here

```sh
python3 tools/livedev-next
```

This prints the repo state, checks that all LiveDev tools are present, and shows the next commands to run.

## Quick reference

| What | Command |
|---|---|
| Full pipeline (auto) | `python3 tools/livedev-next --auto` |
| Check repo state | `python3 tools/livedev-next` |
| Run mock tests | `python3 tools/livedev-next --mock` |
| Generate a plan | `python3 tools/livedev-next --plan` |
| Execute a plan | `python3 tools/livedev-next --run /tmp/rush-livedev-plan.json` |
| Submit evidence (dry-run) | `python3 tools/livedev-next --submit <RUN_DIR> --dry-run` |
| Submit evidence (real) | `python3 tools/livedev-next --submit <RUN_DIR>` |

## What --auto does

1. **Plan** — reads repo state + hardware, generates a typed plan.
2. **Run** — executes the plan through `rush-exec`, captures a tamper-evident session with `rush-capture`. Uses fake mode by default (safe, no real hardware).
3. **Validate** — runs `validate-hwtest-evidence.py` against the generated evidence bundle (14 semantic checks).
4. **Submit (dry-run)** — prepares the evidence PR (branch name, commit message, files) without pushing.

The run directory is printed at the end. To submit for real:

```sh
python3 tools/livedev-next --submit <RUN_DIR>
```

## Mock verification

```sh
python3 tools/livedev-next --mock
```

Runs three end-to-end scenarios (success, failure without AI, failure with AI fix) plus the evidence fixture validation. All in fake/mock mode — no hardware, no network, no real PRs. Takes about 10 seconds.

## Plan generation

```sh
python3 tools/livedev-next --plan
```

Calls `rush-autopilot plan --dry-run --output /tmp/rush-livedev-plan.json`. Reads the repo state and hardware, figures out what needs testing, writes a plan.

## Run evidence

```sh
python3 tools/livedev-next --run /tmp/rush-livedev-plan.json
```

Executes the plan in fake mode (safe, no real hardware). The run directory is printed in the output.

For real hardware runs, use the underlying tool directly:

```sh
python3 tools/rush-autopilot run --plan /tmp/rush-livedev-plan.json --run-dir /tmp/rush-run-001
```

## Validate evidence

```sh
python3 tools/validate-hwtest-evidence.py --bundle <RUN_DIR>
```

Runs 14 semantic checks on the evidence bundle: required files, manifest parses, source version/commit exist, hardware slot valid, laptop battery, battery/AC runs match, baseline/optid paired, sample count, results parse, privacy report, secrets absent, AI not evidence, event chain intact.

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

## What is wired now

- **Planner** (`rush-autopilot plan`) — reads repo state + hardware, generates typed plans. ✅
- **Runner** (`rush-autopilot run`) — executes plans, captures tamper-evident evidence. ✅ (fake mode works; real hardware requires actual hardware)
- **Evidence validator** (`validate-hwtest-evidence.py`) — 14 semantic checks. ✅
- **AI harness** (`rush-agent`) — mock provider for dev-if-fail repair. ✅ (mock only)
- **PR submission** (`rush-autopilot submit-evidence`) — dry-run and real. ✅
- **E2E dry run** (`livedev-e2e-dry-run.py`) — three scenarios. ✅

## What is NOT wired yet

- **Real AI providers** — only mock. Real providers need ADR ratification.
- **Real hardware evidence** — no transcripts submitted. v0.6 criteria remain `verified = false`.
- **LiveDev image boot** — mkosi profile exists, not built on hardware.
- **Milestone close** — separate from evidence PRs, requires maintainer approval.

## What is never automatic

- **No self-merge** — there is no merge command in any LiveDev tool.
- **No milestone verification** — `verified = true` in `release/milestones.toml` is set only by the human maintainer.
- **No release cut** — `VERSION`, `RELEASES.md`, tags are never modified by LiveDev tools.
- **No release-truth edit** — `VERSION`, `Cargo.toml`, `RELEASES.md`, `release/milestones.toml`, `release/test-tiers.toml`, `.github/workflows/ci.yml`, ADR `Status:` lines are all forbidden paths.
- **No fabricated hardware evidence** — all evidence must come from a real run directory.

## testOS compatibility

testOS is NOT replaced or deprecated. It remains the legacy/manual hardware-test path until the LiveDev real-hardware runner fully replaces it.
