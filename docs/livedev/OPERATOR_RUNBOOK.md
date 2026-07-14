# Rush LiveDev Operator Runbook

## Start here — pick a path

LiveDev has two operator paths:

1. **`--run-vm`** (deterministic, QEMU-driven, recommended for CI and dev).
   The host orchestrator launches QEMU, injects test intent, waits for
   explicit guest markers, collects artifacts, and submits results. No
   manual reboot, no USB, no interactive shell. This is the path that
   fixes the unreliable reboot-to-test transition.

2. **`--auto` / `--resume`** (USB-based, for real-hardware benchmark
   campaigns). The host prepares a USB, the operator boots the test
   machine from it, tests run, the machine reboots back, and the host
   resumes by copying results off the USB. This path is preserved for
   real-hardware testing where QEMU cannot run.

### Path 1: `--run-vm` (deterministic QEMU cycle)

```sh
# Build the livedev image (one-time):
sudo bash tools/build-mkosi-image.sh --edition livedev

# Run a smoke test (deterministic, no network needed):
python3 tools/livedev-next --run-vm \
  --image build/rush-linux-livedev.raw \
  --test-cmd 'echo hello && true' \
  --submit-mode local

# Run a CI test (never interactive, always terminates):
python3 tools/livedev-next --run-vm \
  --image build/rush-linux-livedev.raw \
  --test-cmd 'python3 /usr/lib/rush/selftest.py' \
  --ci --submit-mode auto

# Debug a failing test (drops to shell on the guest after failure):
python3 tools/livedev-next --run-vm \
  --image build/rush-linux-livedev.raw \
  --test-cmd 'false' \
  --debug --keep-vm --verbose
```

What `--run-vm` does, step by step:

1. Generates a `run_id` (or uses the one you provide).
2. Creates `artifacts/livedev/<run_id>/` on the host.
3. Writes `metadata.json` (git commit, host kernel, QEMU version).
4. Creates the persistent test-intent state file
   (`/RUSH-DATA/state/livedev-state.json` inside the guest image).
5. Launches QEMU with stable options (`-nographic`, `-no-reboot`,
   `-drive ... if=virtio`, virtio-net user-mode).
6. Captures the serial console to `artifacts/livedev/<run_id>/console.log`.
7. Waits for explicit guest markers — NOT arbitrary sleeps:
   - `RUSH_LIVEDEV_BOOT_READY` (boot phase)
   - `RUSH_LIVEDEV_TEST_START` (test-start phase)
   - `RUSH_LIVEDEV_TEST_PASS` / `RUSH_LIVEDEV_TEST_FAIL` (terminal)
   - `RUSH_LIVEDEV_SHUTDOWN` (clean shutdown)
8. Detects failure patterns (kernel panic, emergency mode, login prompt,
   root shell) and fails the run immediately.
9. Enforces timeouts: boot (180s), test-start (60s), test execution
   (1800s), shutdown (60s). On timeout: marks run failed, saves partial
   logs, force-kills QEMU.
10. Collects artifacts from the guest image after shutdown
    (`/RUSH-DATA/results/livedev/<run_id>/` → host artifacts dir).
11. Writes `summary.json` with status, exit_code, duration, markers,
    git/host metadata.
12. Bundles artifacts into `rush-livedev-results-<run_id>.tar.zst`
    (or `.tar.gz` if zstd is unavailable).
13. Submits results according to `--submit-mode`:
    - `none`: no submission
    - `local`: print artifact path + pass/fail summary
    - `github`: write Markdown to `$GITHUB_STEP_SUMMARY` (CI), post one
      bot PR comment (no spam)
    - `http`: POST summary.json + bundle to `$RUSH_RESULTS_ENDPOINT`
    - `auto`: pick best available
14. Exits with the test exit code (0=pass, 1=fail, 3=boot timeout,
    4=test-start timeout, 5=test timeout, 6=shutdown timeout,
    7=guest failure, 8=state error, 70=infra error).

### Path 2: `--auto` / `--resume` (USB-based, real hardware)

The intended operator path is a single command on a clean workstation. The script clones or fetches the repo, runs mock verification, generates a plan, prepares a USB using the current testOS backend, and tells you when to boot. After the test machine reboots back to its host OS, the same script resumes: it copies results from the USB, validates them, and submits an evidence PR for maintainer review (no auto-merge).

#### Linux/macOS

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh -o livedev-bootstrap.sh && bash livedev-bootstrap.sh --auto
```

#### Windows PowerShell

```powershell
curl.exe -L -o livedev-bootstrap.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.ps1; powershell -ExecutionPolicy Bypass -File .\livedev-bootstrap.ps1 -Auto
```

You only approve USB erase, boot from USB, physical AC/battery prompts, and GitHub auth. The script never auto-merges, never marks milestones verified, and never edits release truth.

If `./Rush-linux` already exists, the bootstrap reuses it when it is a git repo. If it is not a git repo, the bootstrap clones into a timestamped `Rush-linux-livedev-*` directory instead of failing.

## What the USB one-command path does

1. **Clone/fetch repo.** If invoked inside a Rush-linux checkout, uses the current checkout and pulls latest `main`. If `./Rush-linux` already exists and is a git repo, reuses it. If `./Rush-linux` exists but is not a git repo, clones into a timestamped `Rush-linux-livedev-*` alternate directory. Otherwise clones into `./Rush-linux`.
2. **Mock verification.** Runs `python3 tools/livedev-next --mock` (skip with `--skip-mock`). This executes the three end-to-end dry-run scenarios plus the evidence fixture validator. No hardware, no network, ~10 seconds.
3. **Generate plan.** Runs `python3 tools/livedev-next --plan`. The planner reads the repo state plus the host hardware fingerprint and writes `/tmp/rush-livedev-plan.json`.
4. **Prepare USB.** Invokes the testOS installer (`testos/install.sh` on Linux/macOS, `testos/install.ps1` on Windows). The script prints `Using testOS as the current LiveDev boot backend.` because the LiveDev image is not yet wired as a separate boot backend. In `--dry-run` mode, prints the exact command but does not write the USB.
5. **Print reboot instructions.** Exact boot-menu keys, Secure Boot note, testOS menu controls, and the next command to run after reboot (`bash livedev-bootstrap.sh --resume` or `.\livedev-bootstrap.ps1 -Resume`).

After the test machine reboots back to its host OS:

6. **Resume collection** (`--resume`): scans for a removable USB disk, mounts its ESP read-only, and copies `testos-results/<latest>/` into a temp run directory.
7. **Validate results.** Runs a basic manifest schema check (parses, has host fingerprint, has passed/failed/skipped counts). If the bundle has a LiveDev `run-record.json`, runs the full 14-check `validate-hwtest-evidence.py` validator.
8. **Submit dry-run** (default): runs `python3 tools/livedev-next --submit <RUN_DIR> --dry-run`. No push, no PR, no merge.
9. **Submit real** (`--submit`): pushes a branch and opens an evidence PR via the GitHub API. No merge API call is made. A maintainer reviews and merges.

## USB creation

USB creation is part of `--auto`. To prepare a USB without running the full pipeline:

```sh
python3 tools/livedev-next --prepare-usb
```

This calls `bash tools/livedev-bootstrap.sh --auto` (or the PowerShell equivalent on Windows), which in turn invokes `testos/install.sh` / `testos/install.ps1`. The testOS installer:

- Refuses the host's root disk.
- Refuses non-removable disks without `--force`.
- Asks `yes` before any destructive write.
- Verifies SHA256SUMS against the GitHub release assets.

In `--dry-run` mode, prints the exact command but does not write the USB.

## Boot the USB

After USB preparation, the script prints exact reboot instructions:

1. Plug the USB into the test machine.
2. Reboot. Enter the boot menu (F12, F8, F11, or Esc — vendor-dependent).
3. Pick the USB from the boot menu.
4. If it refuses to boot, disable Secure Boot in the BIOS (testOS UKIs are unsigned).
5. testOS boots to a console menu. Type `0` for all benchmarks, or pick specific numbers.
6. Press Esc at any time to abort early (partial results are saved).
7. When tests finish, testOS syncs the USB and auto-reboots back to the host OS.

## Automatic test run

Inside testOS, the `testos-runner` binary boots automatically on tty1, shows the bench menu, runs the selected benchmarks, writes per-benchmark JSON results + a `manifest.json` to the USB, and reboots. No user interaction is required beyond picking tests (or accepting the default `0` = all).

## Resume after reboot

After the test machine reboots back and you plug the USB back into this workstation:

```sh
bash livedev-bootstrap.sh --resume              # Linux/macOS
.\livedev-bootstrap.ps1 -Resume                 # Windows
```

The script:

- Scans for a removable USB disk.
- Mounts its ESP partition read-only.
- Copies `testos-results/<latest>/` into a temp run directory.
- Unmounts the USB.
- Validates the manifest.

If `--dry-run` is passed, prints every step without touching the USB.

## Result validation

The resume step runs these checks:

- `manifest.json` parses as JSON.
- `host.fingerprint` is present.
- `passed`, `failed`, `skipped` arrays are present.
- Counts are printed for human review.

If the run directory also contains a LiveDev `run-record.json` (e.g. when the bundle came from the LiveDev runner rather than testOS), the full `validate-hwtest-evidence.py` validator is invoked. That validator runs 14 semantic checks: required files, manifest parses, source version/commit exist, hardware slot valid, laptop battery, battery/AC runs match, baseline/optid paired, sample count, results parse, privacy report, secrets absent, AI not evidence, event chain intact.

Validation failures do not destroy the run dir. The script keeps it on disk for inspection and tells you to inspect it before submitting.

## Evidence PR submission

### Dry-run (default for `--resume`)

```sh
bash livedev-bootstrap.sh --resume              # already runs submit dry-run by default
```

This runs `python3 tools/livedev-next --submit <RUN_DIR> --dry-run`, which prepares the evidence PR (branch name, commit message, file list) without pushing.

### Real submission

```sh
bash livedev-bootstrap.sh --resume --submit     # Linux/macOS
.\livedev-bootstrap.ps1 -Resume -Submit         # Windows
```

The script:

1. Checks for `GH_TOKEN` (or `GITHUB_TOKEN`). If missing, prints exactly:
   ```
   [TOKEN NEEDED]
   ```
2. Clones the repo shallowly into a temp directory.
3. Copies the run directory into `benchmarks/results/<date>/<host-fingerprint>/`.
4. Commits on a new branch with message `evidence(bench): testOS run <date> host=<fp>`.
5. Pushes the branch using the token (token is never stored in git config).
6. Opens a PR via the GitHub API (`POST /repos/.../pulls`).
7. Prints the PR URL.

No merge API call is made. The PR is opened for maintainer review.

## Token timing

Only provide `GH_TOKEN` when the script prints:

```
[TOKEN NEEDED]
```

Do not set the token in the environment before that point. The token needs: Contents read/write, Pull requests write, Metadata read, Workflows read. The token is never printed, never logged, never stored in git config or on disk.

## No auto-merge

The LiveDev one-command path never calls the GitHub merge API. The PR is opened and left for a maintainer to review and merge. This is enforced in three places:

1. `tools/livedev-bootstrap.sh` and `tools/livedev-bootstrap.ps1` do not invoke `PUT /pulls/{n}/merge`.
2. `tools/rush_pr_lib.py` (`Cannot merge PRs (Human-only)`) is the underlying PR library used by `livedev-next --submit`.
3. `tools/livedev-next --submit` only calls `rush-autopilot submit-evidence`, which uses `rush_pr_lib.py` and inherits its no-merge invariant.

## No milestone verification

`release/milestones.toml` is on the forbidden-paths list for every LiveDev tool. `verified = true` is set only by a human maintainer, never by an automated tool. Evidence PRs may advance toward a milestone exit criterion, but the criterion is not marked verified by the submission.

## No release truth edits

The LiveDev one-command path never edits:

- `VERSION`
- `Cargo.toml`
- `RELEASES.md`
- `release/milestones.toml`
- `release/test-tiers.toml`
- `.github/workflows/ci.yml`
- ADR `Status:` lines

## testOS is the current boot backend

The LiveDev image profile exists in `mkosi/mkosi.profiles/`, but it is not yet built on hardware. Until it is, `livedev-bootstrap.sh` and `livedev-bootstrap.ps1` use testOS as the boot backend and print:

```
Using testOS as the current LiveDev boot backend.
```

testOS is not deprecated and is not removed. It is preserved as both the current boot backend and as a manual fallback path for users who want to drive each step themselves.

## Operator commands inside the repo

If you have already cloned the repo and want finer control, use `tools/livedev-next`:

```sh
python3 tools/livedev-next                       # show the one-command path + repo state
python3 tools/livedev-next --mock                # mock tests (no hardware, ~10s)
python3 tools/livedev-next --auto                # full pipeline: plan -> run -> validate -> submit dry-run
python3 tools/livedev-next --auto --dry-run      # show the full pipeline without writing USB
python3 tools/livedev-next --prepare-usb         # prepare USB using the testOS backend
python3 tools/livedev-next --resume              # resume after reboot
python3 tools/livedev-next --plan                # generate a benchmark plan
python3 tools/livedev-next --run /tmp/rush-livedev-plan.json
python3 tools/livedev-next --submit <RUN_DIR> --dry-run
python3 tools/livedev-next --submit <RUN_DIR>    # real submission (no auto-merge)
python3 tools/livedev-next --help
```

## What is wired now

- **Planner** (`rush-autopilot plan`) — reads repo state + hardware, generates typed plans.
- **Runner** (`rush-autopilot run`) — executes plans, captures tamper-evident evidence. Fake mode works; real hardware requires actual hardware.
- **Evidence validator** (`validate-hwtest-evidence.py`) — 14 semantic checks.
- **AI harness** (`rush-agent`) — mock provider for dev-if-fail repair.
- **PR submission** (`rush-autopilot submit-evidence`) — dry-run and real.
- **E2E dry run** (`livedev-e2e-dry-run.py`) — three scenarios.
- **Bootstrap scripts** (`livedev-bootstrap.sh`, `livedev-bootstrap.ps1`) — one-command USB workflow for Linux/macOS/Windows.
- **`--run-vm` orchestrator** (`rush-livedev-orchestrator`) — deterministic
  QEMU-driven livedev cycle with marker-based state machine, timeout
  enforcement, failure-pattern detection, artifact collection, and
  submission automation.
- **Guest-side test runner** (`rush-livedev-runner` +
  `rush-livedev-test.service`) — post-reboot test execution owned by the
  guest init system, gated on persistent state file, never falls through
  to a root prompt.
- **Persistent test-intent state** (`rush_livedev_state.py`) — atomic
  JSON state at `/RUSH-DATA/state/livedev-state.json` that survives reboot.
- **Console marker protocol** (`rush_livedev_markers.py`) — single-line
  markers the host parses to drive its state machine.
- **Submission automation** (`rush_livedev_submit.py`) — none/local/github/http/auto.

## What is NOT wired yet

- **Real AI providers** — only mock. Real providers need ADR ratification.
- **Real hardware evidence** — no transcripts submitted. v0.6 criteria remain `verified = false`.
- **LiveDev image boot** — mkosi profile exists, not built on hardware. testOS is the current boot backend.
- **Milestone close** — separate from evidence PRs, requires maintainer approval.

## What is never automatic

- **No self-merge** — there is no merge command in any LiveDev tool.
- **No milestone verification** — `verified = true` in `release/milestones.toml` is set only by the human maintainer.
- **No release cut** — `VERSION`, `RELEASES.md`, tags are never modified by LiveDev tools.
- **No release-truth edit** — `VERSION`, `Cargo.toml`, `RELEASES.md`, `release/milestones.toml`, `release/test-tiers.toml`, `.github/workflows/ci.yml`, ADR `Status:` lines are all forbidden paths.
- **No fabricated hardware evidence** — all evidence must come from a real run directory.

## Troubleshooting (`--run-vm` path)

### The system ends at a root prompt instead of running tests

This was the original failure mode that the `--run-vm` path eliminates.
If you still see it, check:

1. **Is the state file present in the image?**
   ```sh
   guestfish -a build/rush-linux-livedev.raw -i ls /RUSH-DATA/state/
   ```
   Should list `livedev-state.json`. If not, the orchestrator's
   `--inject-state copy-on-image` step failed — check the orchestrator
   output for "state injection failed".

2. **Is `rush-livedev-test.service` enabled in the image?**
   ```sh
   guestfish -a build/rush-linux-livedev.raw -i \
     ls /etc/systemd/system/multi-user.target.wants/ | grep rush-livedev-test
   ```
   If not, rebuild the image with the latest `tools/build-mkosi-image.sh`.

3. **Is `getty@tty1.service` masked?**
   ```sh
   guestfish -a build/rush-linux-livedev.raw -i \
     ls -l /etc/systemd/system/getty.target.wants/getty@tty1.service
   ```
   Should be a symlink to `/dev/null` (masked). If it's a real symlink to
   `getty@.service`, the build script didn't mask it — rebuild.

4. **Did the runner crash?** Check `console.log` for
   `RUSH_LIVEDEV_TEST_FAIL exit_code=70` — that's the failure handler
   emitting a marker. If you see it, the runner crashed and the failure
   handler caught it.

### Missing state file

If the orchestrator reports "state file does not exist" or the guest
boots to the autostart countdown instead of running tests:

- The state injection failed silently. Re-run with `--verbose` and look
  for "state injection failed".
- If using `--inject-state none`, the state file must be pre-seeded in
  the image manually. Use `--inject-state copy-on-image` instead.

### Test service did not start

If `BOOT_READY` never appears in `console.log`:

- Check `journal.log` in the artifacts for `rush-livedev-test.service`
  errors.
- Verify the state file is valid: `python3 tools/rush_livedev_state.py
  --path /tmp/state.json validate`.
- Check the `ConditionPathExists` in the service file matches the state
  path you're using.

### Boot timeout

If the run fails with `exit_code=3` (boot timeout):

- Increase `--boot-timeout` (default 180s). Slow CI machines may need 300s.
- Check `console.log` — if it stops early, the image may not boot at all.
  Try booting it manually:
  ```sh
  qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd \
    -drive file=build/rush-linux-livedev.raw,format=raw,if=virtio \
    -m 2G -nographic
  ```

### Test timeout

If the run fails with `exit_code=5` (test execution timeout):

- The test command took longer than `--test-timeout` (default 1800s).
- Increase `--test-timeout` or fix the test command.

### Network unavailable

If submission fails with a network error:

- `--submit-mode local` does NOT require network. Use it for local dev.
- `--submit-mode github` requires the `GITHUB_TOKEN` env var and network
  access to `api.github.com`.
- `--submit-mode http` requires `RUSH_RESULTS_ENDPOINT` to be set.

### Submission failed

If `submit_status` in `summary.json` is `error`:

- Check `submit_error` for the specific failure.
- The run itself still completed — artifacts are in `artifacts/livedev/<run_id>/`.
- You can re-submit manually: `python3 tools/rush_livedev_submit.py
  <artifacts_dir> --run-id <run_id> --submit <mode>`.

### Artifact collection failed

If the artifacts directory is missing guest-side files (`test.log`,
`summary.json`, `guest-diagnostics/`):

- The guest didn't reach `ARTIFACTS_READY`. Check `console.log` for where
  it stopped.
- If using `--inject-state copy-on-image`, the orchestrator copies
  artifacts out by re-mounting the image after shutdown. This requires
  either `guestfish` (libguestfs) or root privileges for loopback mount.
- If neither is available, install `libguestfs` or run the orchestrator
  as root.

## Exit codes (`--run-vm` path)

| Code | Meaning |
|------|---------|
| 0 | Test passed |
| 1 | Test failed (nonzero exit from test command) |
| 2 | Infrastructure failure (QEMU missing, image missing, etc.) |
| 3 | Boot timeout |
| 4 | Test-start timeout (BOOT_READY seen but no TEST_START) |
| 5 | Test-execution timeout (TEST_START seen but no terminal marker) |
| 6 | Shutdown timeout (terminal marker seen but QEMU did not exit) |
| 7 | Unexpected guest failure (root prompt, panic, emergency mode) |
| 8 | State error / invalid intent |
| 70 | Generic infrastructure error |

## Artifact directory structure (`--run-vm` path)

```
artifacts/livedev/<run_id>/
├── summary.json              # host-side summary (status, exit_code, markers, git/host metadata)
├── metadata.json             # written early — exists even on crash
├── console.log               # full serial console capture
├── livedev-state.json        # the state file the host wrote (debug copy)
├── test.log                  # guest-side test command stdout/stderr (copied from guest)
├── summary.json              # guest-side summary (if collection succeeded)
├── guest-diagnostics/
│   ├── dmesg.log
│   ├── journal.log
│   ├── cmdline.txt
│   ├── uname.txt
│   └── os-release.txt
└── (test framework output if produced)

artifacts/livedev/rush-livedev-results-<run_id>.tar.zst   # compressed bundle
```
