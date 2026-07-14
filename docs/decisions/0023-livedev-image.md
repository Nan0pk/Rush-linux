# ADR 0023: Rush LiveDev Image Composition

Status: proposed

> Marked **proposed**; needs human ratification. Scopes the LiveDev image
> profile, its read-only default, and its relationship to testOS.

Date: 2026-07-04
Authors: Z.ai (image-profile phase)
Tags: architecture, livedev, image, mkosi, testos

## Context

The LiveDev track has produced a suite of Python tools (`rush-exec`,
`rush-capture`, `rush-autopilot`, `rush-agent`) and supporting
infrastructure (evidence schemas, validators, redaction, tamper-evident
event chains). Until now these tools have run on any Linux host. Phase 7
makes LiveDev bootable: a minimal Rush Linux image that carries the tools,
boots to `multi-user.target`, runs a safe countdown before autopilot,
and writes only to `/RUSH-DATA/` by default.

testOS is the current hardware-test appliance (single-shot USB boot,
run benchmarks, reboot). LiveDev is the long-term successor path for
continuous operation: running benchmarks, capturing evidence, syncing
with the online repo, and optionally consulting AI providers. testOS
remains available and unmodified during the transition.

## Decision

### 1. LiveDev profile

`mkosi/mkosi.profiles/livedev/mkosi.conf` extends the base `mkosi.conf`
with: `git`, `github-cli`, `python`, `openssh`, network tools, diagnostics,
and `stress`/`rt-tests` for benchmark workloads. No desktop, no audio,
no games. The profile sets `ImageId=rush-linux-livedev` and adds
`livedev.*` kernel cmdline parameters for autostart/countdown/mutate-disk.

### 2. Edition descriptor

`distro/editions/livedev.toml` mirrors the existing `server.toml` shape
(headless, `desktop=false`, `optimizer_mode="balanced"`).

### 3. RUSH-DATA persistent layout

The image uses `/RUSH-DATA/` as the persistent data partition, created
by `systemd-tmpfiles` via `packaging/systemd/rush-livedev-tmpfiles.conf`.
The layout:
- `/RUSH-DATA/repo` — git working tree
- `/RUSH-DATA/state` — capture/autopilot state
- `/RUSH-DATA/results` — evidence bundles
- `/RUSH-DATA/logs` — tool logs
- `/RUSH-DATA/ai` — AI attempt records
- `/RUSH-DATA/secrets` — credentials (0700, never committed)
- `/RUSH-DATA/cache` — package/build cache

### 4. Systemd integration

Three LiveDev-specific units:
- `rush-livedev-autostart.service` — safe countdown on tty1 before
  autopilot. ESC drops to a shell. `livedev.autostart=0` disables.
- `rush-capture.service` — start/stop the capture session.
- `rush-autopilot.service` — generate a plan from repo + hardware state.

These units are enabled in the `00-rush.preset` and symlinked into
`multi-user.target.wants/` by the build script.

### 5. Read-only host disk

The image is read-only on the host disk by default (ADR 0018 §6.3).
The `livedev.mutate_host_disk=1` kernel cmdline flag is required for any
host-disk write. This is the same default testOS enforces.

### 6. testOS compatibility

testOS is NOT modified, deprecated, or replaced. testOS continues to ship
on every `v*` tag. LiveDev is a parallel image. The two coexist.

### 7. No AI on boot

The AI harness (`rush-agent`) is present on the image but NOT auto-invoked
on boot. AI is invoked only through the `dev-if-fail` loop, which is
triggered manually or by the runner when a step fails.

### 8. No PR submission on boot

The GitHub CLI (`gh`) is present on the image but NOT auto-invoked on boot.
PR submission is Phase 8.

## Consequences

### If this ADR is accepted

- `tools/build-mkosi-image.sh --edition livedev` produces a bootable
  `rush-linux-livedev.raw` image.
- The image carries the rush-* tools, evidence validator, schemas, and
  LiveDev systemd units.
- The image boots to a safe countdown; the operator can ESC to a shell
  or wait for autopilot.
- testOS continues to work unchanged.

### If this ADR is rejected

- The LiveDev profile is removed.
- The build script's `--edition livedev` support is removed.
- testOS continues to work unchanged.

## Acceptance criteria

- [ ] `mkosi/mkosi.profiles/livedev/mkosi.conf` exists.
- [ ] `distro/editions/livedev.toml` exists.
- [ ] `tools/build-mkosi-image.sh --help` lists `livedev` as an edition.
- [ ] `packaging/systemd/rush-livedev-tmpfiles.conf` creates the RUSH-DATA layout.
- [ ] `packaging/systemd/rush-livedev-autostart.service` runs the countdown.
- [ ] `packaging/systemd/rush-capture.service` and `rush-autopilot.service` exist.
- [ ] `tools/rush-livedev-autostart` implements the safe countdown + ESC path.
- [ ] `docs/editions/livedev.md` documents the edition.
- [ ] Structural tests pass (profile exists, tools referenced, units valid,
      RUSH-DATA layout works, testOS not broken).
- [ ] `release/milestones.toml`, `VERSION`, `RELEASES.md` are untouched.
- [ ] `testos/` is not modified.

## References

- ADR 0018 (LiveDev architecture contract) — §6.3 read-only host disk.
- `docs/plans/livedev-transition-plan.md` Phase 7.
- `docs/editions/livedev.md`.
- `mkosi/mkosi.profiles/livedev/mkosi.conf`.
- `distro/editions/livedev.toml`.
- `packaging/systemd/rush-livedev-tmpfiles.conf`.
- `tools/rush-livedev-autostart`.
