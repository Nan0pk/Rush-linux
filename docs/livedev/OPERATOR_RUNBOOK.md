# Rush LiveDev Operator Runbook

## Current state

- LiveDev foundation is infrastructure.
- It claims no real hardware evidence.
- Hardware evidence must be submitted in a separate evidence PR.
- Milestone/release truth must be changed only in a separate maintainer-approved PR.

## After clone

Commands:

```sh
git checkout main
git pull --ff-only origin main
python3 tools/livedev-e2e-dry-run.py --success
python3 tools/livedev-e2e-dry-run.py --failure-no-ai
python3 tools/livedev-e2e-dry-run.py --failure-with-ai-fix
python3 tools/validate-hwtest-evidence.py --fixtures
```

## Prepare hardware evidence

Commands:

```sh
git checkout -b evidence/phase-d-hardware-run
python3 tools/rush-autopilot plan --auto --dry-run
python3 tools/rush-autopilot plan --auto --output /tmp/rush-livedev-plan.json
```

## Run hardware evidence

Command:

```sh
python3 tools/rush-autopilot run --plan /tmp/rush-livedev-plan.json
```

The tool output includes the run directory path. Look for the `--run-dir`
value in the output or the `run_dir` field in the printed JSON. The
default is a timestamped directory. If you need to specify it explicitly:

```sh
python3 tools/rush-autopilot run --plan /tmp/rush-livedev-plan.json --run-dir /tmp/rush-run-001
```

After the run completes, validate the evidence bundle:

```sh
python3 tools/validate-hwtest-evidence.py --bundle <RUN_DIR>
```

Where `<RUN_DIR>` is the path printed by the run command.

## Submit evidence

Commands:

```sh
python3 tools/rush-autopilot submit-evidence --run-dir <RUN_DIR> --dry-run
python3 tools/rush-autopilot submit-evidence --run-dir <RUN_DIR>
```

The `--dry-run` flag shows what would be committed (files, branch name,
commit message, PR title/body) without pushing or creating a PR.

The non-dry-run command creates a branch, commits the evidence, pushes,
and opens a PR. It does NOT merge.

## Token timing

Only provide GH_TOKEN when the tool prints:

```
[TOKEN NEEDED]
```

Do not set the token in the environment before that point. The token is
read from the `GH_TOKEN` or `GITHUB_TOKEN` environment variable.

## Never automatic

- No self-merge — there is no merge command in the rush tools.
- No milestone verification — `verified = true` in `release/milestones.toml`
  is set only by the human maintainer.
- No release cut — `VERSION`, `RELEASES.md`, tags are never modified by
  LiveDev tools.
- No release-truth edit — `VERSION`, `Cargo.toml` workspace version,
  `RELEASES.md`, `release/milestones.toml`, `release/test-tiers.toml`,
  `.github/workflows/ci.yml`, ADR `Status:` lines are all forbidden paths.
- No fabricated hardware evidence — all evidence must come from a real
  run directory produced by `rush-autopilot run`.
