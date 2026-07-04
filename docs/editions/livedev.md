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

On boot, `rush-livedev-autostart.service` runs on tty1:

1. Prints a banner with host info + RUSH-DATA status.
2. Starts a countdown (default 10 seconds, configurable via
   `livedev.countdown_sec=N` kernel cmdline).
3. If ESC is pressed, drops to a bash shell (the escape/menu path).
4. If the countdown completes, exits 0; systemd proceeds to start
   `rush-capture.service` + `rush-autopilot.service`.

To disable autostart: add `livedev.autostart=0` to the kernel cmdline.

## Systemd units

| Unit | Description |
|---|---|
| `rush-livedev-autostart.service` | Safe countdown on tty1 before autopilot |
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
