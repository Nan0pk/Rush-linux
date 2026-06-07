# Roadmap

Current project version: `0.3.0-alpha.1`

The detailed v1 release plan lives in `docs/release-plan-v1.md`. The
machine-readable milestone gates live in `release/milestones.toml`.

## Phase 0: Repository Foundation

Status: complete.

- GitHub repository exists.
- Documentation layer and ADRs exist.
- Future-facing default validation exists.
- Release governance exists.

Exit criteria:

- `tools/validate-repo.ps1` passes.
- Release versioning, checklist, and milestone docs exist.

## v0.1.0-alpha.1: Compile-Clean Core
 
Status: complete.
 
- Make `optid` and `optctl` compile cleanly.
- Run `cargo fmt`, `cargo test`, and `cargo clippy -D warnings` in CI.
- Add fixture-based tests for PSI, battery, thermal, and policy decisions.
- Keep status, roadmap, and continuation docs aligned.
 
Exit criteria:
 
- CI green on GitHub.
- `optid --once` works on Linux in dry-run mode.
- `optctl status`, `explain`, `mode`, `trace`, and `benchmark` compile.
- No docs drift.
 
## v0.2.0-alpha.1: Real Control Plane
 
Status: complete.
 
- Implement `io.rushlinux.Optid1` D-Bus server in `optid`.
- Implement D-Bus client calls in `optctl`.
- Keep `/run/optid` files as diagnostics and recovery state.
- Parse `config/optid/policy.toml`.
- Add `optctl status --json`.
 
Exit criteria:
 
- Mode changes work through D-Bus.
- `optctl explain` reports last decision with reasons.
- Invalid config fails safely.
- D-Bus docs match implementation.
 
## v0.3.0-alpha.1: Rootfs And Package Builder MVP
 
Status: complete.
 
- Define recipe schema v0.
- Build a minimal rootfs from recipes.
- Generate package metadata locally.
- Add package signing stub.
- Produce a bootable VM disk image before ISO images.
 
Exit criteria:
 
- Minimal VM boots to `multi-user.target`.
- `optid.service` starts.
- `nftables.conf` loads.
- cgroup v2 and PSI are active.
 
## v0.4.0-alpha.1: UKI, Boot, Rollback, Updates
 
Status: next.

- Generate UKIs from kernel package outputs.
- Add systemd-boot path and GRUB fallback path.
- Implement systemd-sysupdate descriptors against local artifacts.
- Add boot assessment and rollback entries.

Exit criteria:

- VM boots through UKI.
- At least three rollback entries are retained.
- Simulated bad kernel rolls back.
- Test update metadata is signed.

## v0.5.0-beta.1: Minimal Installable System

Status: planned.

- Produce minimal ISO or VM installer image.
- Add filesystem selection.
- Add server/minimal edition profile.
- Add smoke tests for install, boot, update, rollback, optid dry-run, and
  network.

Exit criteria:

- Fresh install succeeds in VM.
- Installed system boots twice cleanly.
- Update and rollback tests pass.
- No desktop stack required for minimal/server edition.

## v0.6.0-beta.1: Hardware-Aware optid

Status: planned.

- Add CPU topology, EPP, platform profile, storage class, thermal, and battery
  detection.
- Add hardware allowlist database for risky sysfs knobs.
- Add memory pressure policy.
- Add background cgroup throttling policy.
- Add foreground session detection through systemd/logind first.

Exit criteria:

- Unsupported knobs are skipped with logged reasons.
- Mixed-load responsiveness improves on at least two machines.
- Battery behavior matches or improves mainstream defaults on at least one
  laptop.
- No unsafe write occurs outside allowlisted paths.

## v0.7.0-beta.1: Editions

Status: planned.

- Build desktop KDE Plasma Wayland image.
- Build laptop profile.
- Build realtime audio profile with optional PREEMPT_RT kernel.
- Build server image with systemd-networkd option and nftables.
- Validate edition defaults against ADRs.

Exit criteria:

- Each edition installs and boots.
- Desktop uses Wayland and PipeWire by default.
- Realtime edition uses RT kernel only when selected.
- Server does not install desktop stack by default.

## v0.8.0-beta.1: Benchmark Lab

Status: planned.

- Implement benchmark harness from `benchmarks/manifest.toml`.
- Compare against Fedora current, Ubuntu current, Arch current, and minimal
  tuned baseline.
- Capture optimizer decisions alongside benchmark metrics.
- Publish benchmark report format.
- Add regression thresholds.

Exit criteria:

- Public benchmark artifact generated.
- Regressions block release candidates.
- `optctl explain` correlates with benchmark traces.

## v0.9.0-rc.1: Release Candidate Hardening

Status: planned.

- Freeze v1 recipe schema, policy schema, D-Bus API, and edition names.
- Complete signed package metadata.
- Complete update and rollback testing.
- Run security review of privileged `optid` actions.
- Add release notes and known issues.

Exit criteria:

- No known data-loss, boot-failure, privilege-escalation, or rollback blockers.
- Fresh install and upgrade tests pass.
- CI, VM, hardware, benchmark, and security gates pass.
- Documentation is complete and matches behavior.

## v1.0.0: Stable

Status: planned.

- Publish minimal/server, desktop, laptop-capable, and realtime audio artifacts.
- Publish source recipes, signed package metadata, UKI/kernel artifacts,
  benchmark report, release notes, and rollback guide.

Exit criteria:

- Installable on mainstream x86_64 hardware and VMs.
- `optid` is active, explainable, and reversible.
- Stable update channel exists.
- Rollback works for bad kernel/update scenarios.
- Benchmarks show better mixed-load responsiveness and competitive battery
  behavior.
