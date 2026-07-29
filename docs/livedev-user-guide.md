# Rush LiveDev — User Guide

> **Status:** skeleton (e2e phase). The LiveDev image has not yet been
> booted on real hardware. This guide describes the intended workflow.

## What is LiveDev?

Rush LiveDev is a minimal bootable Rush Linux image that runs
benchmarks, captures evidence, syncs with the online repo, and
optionally consults AI providers for repair. It is the long-term
successor to testOS for continuous operation.

## Creating a LiveDev USB

```sh
# Build the image (requires Arch host + mkosi)
sudo bash tools/build-mkosi-image.sh --edition livedev

# Write to USB (replace /dev/sdX with your USB device)
dd if=build/rush-linux-livedev.raw of=/dev/sdX bs=4M status=progress
```

### Retrying interrupted USB preparation

The one-command bootstrap keeps a persistent checkpoint outside `/tmp`.
If USB preparation fails after the inventory or plan has already been saved,
run the same bootstrap command again. Replayed earlier-phase saves for the
same active run are treated as idempotent retries: the later checkpoint,
existing plan, inventory, and run ID remain authoritative. Submitted runs and
attempts to replace an active checkpoint with a different run ID remain
fail-closed.

## Booting LiveDev

1. Insert the USB into the target machine.
2. Boot from USB (select it in the firmware boot menu).
3. The system boots to `multi-user.target` with `optid.service` active.
4. On tty1, a countdown appears:

```
============================================================
  Rush Linux LiveDev — Autostart
============================================================
  Host:    <hostname>
  Kernel:  <kernel-version>
  RUSH-DATA: /RUSH-DATA (ready)

  Autopilot starts in 10 seconds.
  Press ESC for a shell, or wait for autopilot.
```

5. **Press ESC** to drop to a shell (escape/menu path), or **wait** for
   autopilot to start.

## What runs automatically

If you wait for the countdown:

1. `rush-livedev-autostart.service` exits 0.
2. `rush-capture.service` starts a capture session in `/RUSH-DATA/state/capture/`.
3. `rush-autopilot.service` generates a plan from repo + hardware state.
4. The plan is saved to `/RUSH-DATA/state/plan.json`.

The plan is NOT auto-executed. The operator reviews it and runs it manually:

```sh
rush-autopilot run --plan /RUSH-DATA/state/plan.json --run-dir /RUSH-DATA/state/capture
```

## What humans are asked

### AC/battery prompts

If the plan requires a battery run, the runner displays:

```
[wait] Unplug the laptop's AC adapter to run on battery.
       Detecting: read /sys/class/power_supply/AC*/online == 0 every 5s.
       Reason: Criterion 3 (battery behavior) requires a battery-only run.
       Timeout: 5m.
```

The runner polls the AC online state. When it detects AC is unplugged
(online == 0), it proceeds automatically. No "press Enter" prompt.

### GitHub authentication

If the plan includes PR submission, the runner needs a GitHub token:

```sh
# Place the token at the well-known path (0600 permissions)
echo 'GITHUB_TOKEN=ghp_your_token_here' > /RUSH-DATA/secrets/github.env
chmod 0600 /RUSH-DATA/secrets/github.env
```

The token is never logged, echoed, or committed.

### AI approval

If a run fails and `--dev-if-fail` is set, the AI harness:

1. Builds a redacted context bundle (all secrets removed).
2. Calls the mock provider (or a ratified online provider).
3. Receives a diagnosis + patch.
4. Validates the patch (no forbidden paths, no destructive patterns).
5. Runs validation through rush-exec.

The AI never executes shell commands directly. The AI's patch is
validated and applied by the runner, not by the AI. The AI's verdict
is advisory only — it never marks evidence as pass.

### What data may go to model providers

If a ratified online AI provider is configured:

- The **redacted context bundle** is sent to the provider. This includes:
  failing run summary, manifest, plan, validator output, log excerpts,
  command-log excerpts, source files, git diff, hardware metadata.
- **All secrets are redacted** before sending (GitHub tokens, API keys,
  bearer tokens, MAC addresses, serials, private IPv4, SSH keys).
- The provider's response is a text diagnosis + patch. It is written to
  a file, not executed.
- **No data is sent without the operator's knowledge.** The `--provider`
  flag must be explicitly set.

## Evidence PR flow

1. The runner produces an evidence bundle in the run directory.
2. `rush-autopilot submit-evidence --run-dir <path> --dry-run` shows what
   would be committed.
3. `rush-autopilot submit-evidence --run-dir <path>` (without `--dry-run`)
   creates a branch, commits the evidence, pushes, and opens a PR.
4. The PR body includes: goal, plan, execution record, evidence paths,
   inferred verdict, and "Awaiting Verifier review."
5. **The PR opens for maintainer review.** The maintainer reviews and merges.

## Code PR flow

1. `rush-autopilot submit-code-pr --branch <name> --dry-run` shows what
   would be committed.
2. `rush-autopilot submit-code-pr --branch <name>` creates a PR.
3. CI runs all validation checks (schema, evidence, privacy, release truth).
4. **The PR opens for maintainer review.** The maintainer reviews and merges.

## What is NEVER automated

- **Merging PRs** — only the human maintainer merges to `main`.
- **Marking milestones verified** — only the human maintainer flips
  `verified = true` in `release/milestones.toml`.
- **Modifying release truth** — VERSION, RELEASES.md, milestones.toml,
  ADR Status lines, CI workflows are all forbidden.
- **Deleting tests** — forbidden by the patch validator.
- **Weakening validators** — forbidden by the patch validator.
- **Privileged shell commands** — forbidden by the patch validator.
- **Network calls in tests** — forbidden by the patch validator.
- **Self-merge** — there is no merge command in the rush tools.
- **Host disk mutation** — the image is read-only on the host disk by
  default (`--mutate-host-disk` flag required).

## testOS compatibility

testOS is NOT replaced by LiveDev. testOS remains the "Try it on real
hardware" target. LiveDev is a parallel image for continuous operation.
The two coexist.
