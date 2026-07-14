# Release Plan To v1.0.0

Target final release: `v1.0.0`, the first stable Rush Linux release.

Rush Linux reaches `v1.0.0` only when it has a bootable, installable,
rollbackable Linux distro with a working `optid` adaptive optimizer, signed
package/update flow, edition profiles, documented defaults, and benchmark
evidence against mainstream distros.

## v0.1.0-alpha.1: Compile-Clean Core

Goal: prove the Rust and policy scaffold builds.

Actions:

- Make GitHub Actions pass for format, tests, clippy, and repository policy.
- Add fixture-based tests for PSI parsing, battery detection, thermal detection,
  and policy decisions.
- Keep `tools/validate-repo.ps1` as required CI.
- Keep status, roadmap, and continuation docs aligned.

Exit criteria:

- CI green on GitHub.
- `optid --once` works on Linux in dry-run mode.
- `optctl status`, `explain`, `mode`, `trace`, and `benchmark` compile.
- No docs drift.

## v0.2.0-alpha.1: Real Control Plane

Goal: replace file-only control with the accepted D-Bus API.

Actions:

- Implement `io.rushlinux.Optid1` D-Bus server in `optid`.
- Implement D-Bus client calls in `optctl`.
- Keep `/run/optid` files as diagnostics and recovery state.
- Parse `config/optid/policy.toml` instead of hardcoded thresholds.
- Add `optctl status --json`.

Exit criteria:

- `optctl mode balanced|battery|performance|realtime|auto` works through D-Bus.
- `optctl explain` reports the last decision with reasons.
- Invalid config fails safely and keeps dry-run behavior.
- D-Bus docs match implementation.

## v0.3.0-alpha.1: Rootfs And Package Builder MVP

Goal: build a minimal Rush Linux rootfs from recipes.

Actions:

- Define recipe schema v0.
- Implement a builder that installs systemd, kernel artifacts, nftables, optid,
  configs, and docs into a rootfs.
- Generate local package metadata.
- Add package signing stub.
- Produce a bootable VM disk image before ISO images.

Exit criteria:

- Minimal VM boots to `multi-user.target`.
- cgroup v2 and PSI are active.
- `optid.service` starts.
- `nftables.conf` loads.
- Rootfs build can compare file manifests for reproducibility.

## v0.4.0-alpha.1: UKI, Boot, Rollback, Updates

Goal: make bad kernel/update recovery real.

Actions:

- Generate UKIs from kernel package outputs.
- Add systemd-boot path and GRUB fallback path.
- Implement systemd-sysupdate descriptors against local artifacts.
- Add boot assessment and rollback entries.
- Document Secure Boot and measured boot as supported-but-not-final unless
  signing is production-ready.

Exit criteria:

- VM boots through UKI.
- At least three rollback entries are retained.
- Simulated bad kernel rolls back.
- Update metadata is signed in test mode.
- Boot/update docs match behavior.

## v0.5.0-beta.1: Minimal Installable System

Goal: first installable beta for minimal/server use.

Actions:

- Produce minimal ISO or VM installer image.
- Add filesystem selection: Btrfs default, XFS server option.
- Add server/minimal edition profile.
- Add smoke tests for install, boot, update, rollback, optid dry-run, and
  network.

Exit criteria:

- Fresh install succeeds in VM.
- Installed system boots twice cleanly.
- Update and rollback tests pass.
- No desktop stack is required for minimal/server edition.

## v0.6.0-beta.1: Hardware-Aware optid

Goal: make adaptive policy meaningful on real machines.

Actions:

- Add CPU topology, EPP, platform profile, storage class, thermal, and battery
  detection.
- Add hardware allowlist database for risky sysfs knobs.
- Add memory pressure policy for zswap/zram behavior.
- Add background cgroup throttling policy.
- Add foreground session detection through systemd/logind first.

Exit criteria:

- Unsupported knobs are skipped with logged reasons.
- Mixed-load responsiveness improves on at least two test machines.
- Battery behavior matches or improves mainstream defaults on at least one
  laptop.
- No unsafe write occurs outside allowlisted paths.

## v0.7.0-beta.1: Desktop, Laptop, Realtime, Server Editions

Goal: produce the supported edition set.

Actions:

- Build desktop KDE Plasma Wayland image.
- Build laptop profile with suspend, display, runtime PM, and thermal policy.
- Build realtime audio profile with optional PREEMPT_RT kernel.
- Build server image with systemd-networkd option and nftables.
- Validate edition defaults against ADRs.

Exit criteria:

- Each edition installs and boots.
- Desktop uses Wayland and PipeWire by default.
- Realtime edition uses RT kernel only when selected.
- Server does not install desktop stack by default.
- Edition docs and recipes match.

## v0.8.0-beta.1: Benchmark Lab

Goal: turn performance claims into release gates.

Actions:

- Implement benchmark harness from `benchmarks/manifest.toml`.
- Compare against Fedora current, Ubuntu current, Arch current, and minimal
  tuned baseline.
- Capture optimizer decisions alongside benchmark metrics.
- Publish benchmark report format.
- Add regression thresholds.

Exit criteria:

- Public benchmark artifact is generated.
- Regressions block release candidates.
- `optctl explain` correlates with benchmark traces.

## v0.9.0-rc.1: Release Candidate Hardening

Goal: freeze defaults and stabilize the upgrade path.

Actions:

- Freeze v1 recipe schema, policy schema, D-Bus API, and edition names.
- Complete signed package metadata.
- Complete update and rollback testing.
- Run security review of privileged `optid` actions.
- Add release notes and known issues.
- Stop adding features except release blockers.

Exit criteria:

- No known data-loss, boot-failure, privilege-escalation, or rollback blockers.
- Fresh install and upgrade tests pass.
- CI, VM, hardware, benchmark, and security gates pass.
- Documentation is complete and matches behavior.

## v1.0.0: Final Stable Release

Goal: first stable Rush Linux release.

Release artifacts:

- Minimal/server image.
- Desktop image.
- Laptop-capable desktop image/profile.
- Realtime audio image/profile.
- Source recipes.
- Signed package repo metadata.
- UKI/kernel artifacts.
- Benchmark report.
- Release notes.
- Upgrade/rollback guide.
- Security policy.

Final release criteria:

- Installable on mainstream x86_64 hardware and VMs.
- ARM64 support is documented as experimental unless fully tested.
- `optid` is active, explainable, and reversible.
- Stable update channel exists.
- Rollback works for bad kernel/update scenarios.
- Benchmarks show better mixed-load responsiveness and competitive battery
  behavior.
- No obsolete defaults are introduced.
- Docs are complete enough for a new engineer or AI agent to continue safely.

