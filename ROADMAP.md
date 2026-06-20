# Roadmap

Current project version: `0.5.0-beta.1`

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

Status: complete. (Verified 2026-06-08: VM boots to multi-user.target with optid.service active.)
 
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

Status: complete. All four exit criteria verified (2026-06-08–2026-06-20).
VM boots through OVMF/systemd-boot/UKI, rollback entries are retained,
simulated bad kernel rolls back, and update metadata is signed.

- Generate UKIs from kernel package outputs. ✅
- Add systemd-boot path and GRUB fallback path. ✅
- Implement systemd-sysupdate descriptors against local artifacts. ✅
- Add boot assessment and rollback entries. ✅
- Manage rollback entry retention (≥3 entries). ✅
- Sign update metadata with test Ed25519 keys. ✅
- Simulate bad-kernel rollback and verify recovery. ✅

Exit criteria:

- VM boots through UKI. ✅ (verified 2026-06-08)
- At least three rollback entries are retained. ✅ (tools/manage-boot-entries.sh)
- Simulated bad kernel rolls back. ✅ (tools/test-rollback.sh)
- Test update metadata is signed. ✅ (tools/sign-updates.sh / sign_updates.py)

## v0.5.0-beta.1: Minimal Installable System

Status: in progress.

- Re-base the image composition plane on `mkosi` with an Arch Linux package base.
- Retire `tools/rush-builder.py` and `recipes/` once parity is proven.
- Complete Wave 0 (zram-generator, systemd-oomd, MGLRU) and Wave 1 (mkosi/Arch base, scx + scx_loader + optid integration).

Exit criteria:

- mkosi/Arch image passes all existing `validate-uefi-boot.sh` and `test-rollback.sh` checks unmodified.
- scx soak passes with EEVDF fallback verified.

## v0.6.0-beta.1: Hardware-Aware optid

Status: planned.

- Implement Wave 2 features: compatibility D-Bus interfaces (PPD, GameMode), TLP allowlist, foreground detection, and vm.* actuation.
- Add hardware allowlist database (`config/optid/hardware-allowlist.toml`).
- Prevent conflicting daemons (TLP, tuned, ppd) from executing alongside `optid`.

Exit criteria:

- GNOME/KDE power slider and games drive optid mode changes via D-Bus shims.
- No writes occur outside allowlisted paths.
- Unsupported knobs are skipped with logged reasons.

## v0.7.0-beta.1: Editions

Status: planned.

- Implement editions as mkosi profiles and signed system extensions (sysexts) on a single base image.
- Build profiles for desktop, laptop, and realtime-audio (PREEMPT_RT kernel).

Exit criteria:

- Each edition installs, boots, and verifies cleanly.
- Desktop uses Wayland, PipeWire, and default-on sched_ext.
- Server profile builds without desktop components.

## v0.8.0-beta.1: Benchmark Lab

Status: planned.

- Implement Benchmark Lab backed by Phoronix Test Suite (Wave 3).
- Compare performance-per-watt and latency metrics against mainstream Linux distributions.

Exit criteria:

- Public benchmark artifact generated automatically from tests.
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
