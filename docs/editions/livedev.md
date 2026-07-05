# LiveDev Edition

> **Status:** skeleton (image-profile phase). The LiveDev profile exists
> and is structurally valid; the image has not yet been booted on real
> hardware. See `docs/plans/livedev-progress.json` for the current phase.

The LiveDev edition is a minimal bootable Rush Linux skeleton for
continuous LiveDev operation. It is the long-term successor path to
testOS for the workflows that testOS's single-shot appliance model
cannot serve: continuous running, network sync, PR submission, and
optional AI-assisted development.

## What LiveDev IS

- A minimal bootable Rush Linux image built from the same `mkosi.conf`
  base as the server edition, plus a LiveDev-specific profile
  (`mkosi/mkosi.profiles/livedev/mkosi.conf`).
- Carries the rush-* tools: `rush-exec`, `rush-capture`, `rush-autopilot`,
  `rush-agent`, and `rush-livedev-autostart`.
- Carries the evidence validator: `validate-hwtest-evidence`.
- Carries `git`, `gh` (GitHub CLI), Python, and network tooling.
- Boots to `multi-user.target` with `optid.service` active.
- Runs a safe countdown on tty1 before autopilot starts; pressing ESC
  drops to a shell (the escape/menu path).
- Writes only to `/RUSH-DATA/` (the persistent data partition) by default.
- Does NOT write to the host disk unless `--mutate-host-disk` is explicitly
  set (ADR 0018 §6.3).

## What LiveDev is NOT

- Not a consumer distribution. No desktop, no audio, no games.
- Not a replacement for testOS. testOS remains the "Try it on real
  hardware" target until a follow-up ADR declares otherwise.
- Not self-verified. The LiveDev runner does not mark milestones verified.
- Not self-merging. The LiveDev runner does not merge PRs.
- Not AI-autonomous. The AI harness is present but not auto-invoked on boot.

## Build

```sh
sudo bash tools/build-mkosi-image.sh --edition livedev
```

Output: `build/rush-linux-livedev.raw`

## Boot

```sh
# QEMU
qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd \
  -drive file=build/rush-linux-livedev.raw,format=raw,if=virtio \
  -m 2G -nographic

# USB
dd if=build/rush-linux-livedev.raw of=/dev/sdX bs=4M status=progress
```

## RUSH-DATA persistent layout

The LiveDev image uses a persistent data partition at `/RUSH-DATA/`:

```
/RUSH-DATA/
├── repo/       — git working tree (the Rush Linux repo clone)
├── state/      — rush-capture/rush-autopilot state files
├── results/    — evidence bundles (validated by validate-hwtest-evidence)
├── logs/       — rush-exec/rush-capture/rush-agent logs
├── ai/         — AI attempt records (dev-if-fail)
├── secrets/    — provider credentials, signing keys (never committed, 0700)
└── cache/      — package cache, build artifacts
```

The directory structure is created by `systemd-tmpfiles` via
`packaging/systemd/rush-livedev-tmpfiles.conf`. The partition itself is
expected to be mounted by a systemd mount unit or fstab entry.

## Autostart behavior

On boot, the LiveDev image follows one of two paths depending on whether
a persistent test-intent state file exists at
`/RUSH-DATA/state/livedev-state.json`:

### Idle boot (no state file)

`rush-livedev-autostart.service` runs on tty1:

1. Prints a banner with host info + RUSH-DATA status.
2. Starts a countdown (default 10 seconds, configurable via
   `livedev.countdown_sec=N` kernel cmdline).
3. If ESC is pressed, drops to a bash shell (the escape/menu path).
4. If the countdown completes, exits 0; systemd proceeds to start
   `rush-capture.service` + `rush-autopilot.service` (which generates a
   plan but does NOT execute it — that requires the test runner below).

To disable autostart: add `livedev.autostart=0` to the kernel cmdline.

### Test boot (state file present)

`rush-livedev-test.service` runs (the autostart countdown is skipped via
`ConditionPathExists=!/RUSH-DATA/state/livedev-state.json` on the autostart
unit and `Conflicts=rush-livedev-autostart.service` on the test unit):

1. Reads `/RUSH-DATA/state/livedev-state.json` (the persistent test intent).
2. Validates the state (mode, run_id, attempt_count).
3. Emits `RUSH_LIVEDEV_BOOT_READY run_id=<run_id>` on the console.
4. Sets state.status = "running".
5. Emits `RUSH_LIVEDEV_TEST_START run_id=<run_id>`.
6. Runs `state.test_command` via `/bin/sh -c`, capturing stdout/stderr to
   `/RUSH-DATA/results/livedev/<run_id>/test.log`.
7. Captures exit_code, writes `summary.json`, collects dmesg/journal/uname.
8. Emits `RUSH_LIVEDEV_TEST_PASS` or `RUSH_LIVEDEV_TEST_FAIL exit_code=<N>`.
9. Emits `RUSH_LIVEDEV_ARTIFACTS_READY path=<path>`.
10. Emits `RUSH_LIVEDEV_SHUTDOWN`.
11. Powers off the system.

In `--debug` mode (state.debug=true), on failure the runner instead emits
`RUSH_LIVEDEV_DEBUG_SHELL` and execs an interactive bash on the current
tty. In `--ci` mode (state.ci=true), the runner NEVER goes interactive and
ALWAYS terminates by pass/fail/timeout.

If `rush-livedev-test.service` fails (e.g. the runner crashes),
`rush-livedev-failure.service` is triggered via `OnFailure=`. It emits a
`RUSH_LIVEDEV_TEST_FAIL` marker with `exit_code=70` and powers off — it
never leaves the system at a bare root prompt.

## Why no root prompt

The LiveDev image masks `getty@tty1.service` (see
`tools/build-mkosi-image.sh`). The tty1 console is owned by either:

- `rush-livedev-autostart.service` (idle boot — countdown + ESC menu), or
- `rush-livedev-test.service` (test boot — runs tests + powers off).

A bare root login prompt on tty1 is the failure mode this design
eliminates: it leaves the operator staring at a shell with no test status,
no markers, and no way for the host orchestrator to detect what happened.
The autostart service still drops to bash on ESC for debugging, but that
is an explicit operator action, not a default.

## Console marker protocol

The guest emits single-line markers on the serial console (which the host
captures via QEMU's `-nographic` mode). The host orchestrator drives its
state machine off these markers — it does NOT use arbitrary sleeps.

| Marker | Meaning |
|---|---|
| `RUSH_LIVEDEV_BOOT_READY run_id=<id>` | Guest booted, runner started |
| `RUSH_LIVEDEV_TEST_START run_id=<id>` | Runner is about to execute the test command |
| `RUSH_LIVEDEV_TEST_PASS run_id=<id>` | Test command exited 0 |
| `RUSH_LIVEDEV_TEST_FAIL run_id=<id> exit_code=<N>` | Test command exited nonzero |
| `RUSH_LIVEDEV_ARTIFACTS_READY run_id=<id> path=<path>` | Artifacts directory is populated |
| `RUSH_LIVEDEV_SHUTDOWN run_id=<id>` | Guest is about to power off cleanly |
| `RUSH_LIVEDEV_DEBUG_SHELL run_id=<id>` | Guest is intentionally dropping to a shell (--debug) |

The host also detects UNINTENDED guest patterns as failures:
- kernel panic / oops / call trace
- emergency mode / rescue mode
- "Give root password for maintenance"
- `login:` prompt appearing BEFORE `BOOT_READY`
- root shell prompt (`root@...:~#`, `bash-5.1#`, `~#`)
- systemd failed unit (`Job for X.service failed`)

## Systemd units

| Unit | Description |
|---|---|
| `rush-livedev-test.service` | Post-reboot test runner (gated on state file) |
| `rush-livedev-failure.service` | Fail-closed handler (emits TEST_FAIL, powers off) |
| `rush-livedev-autostart.service` | Safe countdown on tty1 (idle boot only) |
| `rush-capture.service` | Start/stop the capture session |
| `rush-autopilot.service` | Generate a plan from repo + hardware state |
| `optid.service` | Adaptive optimization daemon (dry-run default) |

## testOS compatibility

testOS is NOT modified, deprecated, or replaced by LiveDev. testOS
continues to ship on every `v*` tag via `.github/workflows/release-testos.yml`.
LiveDev is a parallel image for continuous operation; testOS is a
single-shot appliance for benchmark campaigns. The two serve different
purposes and coexist.

## Read-only host disk

By default, the LiveDev image does NOT write to the host's permanent disk.
All writes go to `/RUSH-DATA/` (the persistent data partition) or `/run/`
(volatile). An explicit `--mutate-host-disk` boot flag
(`livedev.mutate_host_disk=1` kernel cmdline) is required for any
host-disk write. This is the same default testOS enforces.

## Security

- No production signing keys on the image (test keys only).
- No secrets in the image (provider credentials injected at runtime).
- Outbound-only network by default (HTTPS to github.com + ratified AI
  provider). Inbound refused by `nftables.conf`.
- SSH inbound is opt-in via a boot flag.
