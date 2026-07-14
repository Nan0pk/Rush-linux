# Roadmap

Current project version: `0.7.0-beta.4`

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

Status: complete. All four exit criteria verified with committed transcripts
in `release/evidence/v0.3.0-alpha.1/` (PR #174, 2026-06-23). The earlier
Dragnet-001 finding G3 — that the 2026-06-08 build-host assertions lacked
committed transcripts — is closed.
 
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

Status: complete. All four exit criteria verified with committed transcripts
in `release/evidence/v0.4.0-alpha.1/` (PR #174, 2026-06-23). The earlier
Dragnet-001 finding G3 — that the UKI-boot and rollback assertions lacked
committed transcripts — is closed.

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

Status: complete. All four exit criteria verified with committed transcripts
in `release/evidence/v0.5.0-beta.1/` (PR #174, 2026-06-23). The mkosi/Arch
image, edition profiles, installer flow, and test harness are implemented
and acceptance-tested. The canonical, machine-readable state lives in
`release/milestones.toml`; this section follows it.

Exit criteria (from `release/milestones.toml`):

- fresh VM install succeeds — verified (transcript: `c1-fresh-install/`).
- installed system boots twice cleanly — verified (transcript: `c2-double-boot/`).
- update and rollback tests pass — verified (transcript: `c3-update-rollback/`).
- server edition has no desktop dependency — verified (static + built-image
  confirmation; transcript: `c4-server-no-desktop/`).

## v0.6.0-beta.1: Hardware-Aware optid

Status: code-complete, **certification pending Phase D**. The in-container Work
Packages are merged: PPD shim (PR #183), GameMode shim (PR #184), `vm.guest`
workload class (PR #185), and foreground-detection stub (PR #186). See
`docs/plans/v0.6-hardware-aware-optid-proposal.md` for the implementation plan.
The two **quantitative** exit criteria (responsiveness on two machines, battery
behavior) are hardware-gated and are tracked in Phase D — see
`docs/strategy/reference-hardware.md` (D1) and
`docs/strategy/mixed-load-workload.md` (D2). The canonical, machine-readable
state lives in `release/milestones.toml`; this section follows it.

- Implement Wave 2 features: compatibility D-Bus interfaces (PPD, GameMode), TLP allowlist, foreground detection, and vm.* actuation.
- Add hardware allowlist database (`config/optid/hardware-allowlist.toml`).
- Prevent conflicting daemons (TLP, tuned, ppd) from executing alongside `optid`.

Exit criteria (from `release/milestones.toml`):

- unsupported knobs are skipped with reasons — code-complete (PRs #183–#186).
- mixed-load responsiveness improves on two machines — **pending Phase D** (D3–D5).
- battery behavior matches or improves mainstream defaults — **pending Phase D** (D3–D5).
- no unsafe write occurs outside allowlisted paths — code-complete (`guarded_write`).

## v0.7.0-beta.1: Editions

Status: in progress (current version). Phase E version bump landed; edition
work packages follow.

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
