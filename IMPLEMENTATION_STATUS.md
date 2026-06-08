# Implementation Status

Last updated: 2026-06-08

## Overall State

The repository is an early implementation scaffold, not a bootable distro yet.
It defines the architecture, accepted defaults, Rust optimizer MVP, config
fragments, packaging skeletons, CI, and documentation layer needed for the next
engineering milestones.

## Implemented

- Current project version recorded in `VERSION` as `0.4.0-alpha.1`.
- Release plan, versioning rules, release policy, release checklist, and
  machine-readable milestone/test-tier manifests.
- Documentation policy requiring future changes to document purpose, impact,
  validation, safety implications, and follow-up work.
- Git repository initialized locally on `main`.
- GitHub remote configured for `https://github.com/Nan0pk/Rush-linux.git`.
- Apache-2.0 license, CI workflow, security policy, and publishing helper.
- `optid` Rust MVP:
  - reads PSI from `/proc/pressure/{cpu,memory,io}`;
  - reads AC/battery state from `/sys/class/power_supply`;
  - reads thermal state from `/sys/class/thermal`;
  - reads load average from `/proc/loadavg`;
  - decides battery, balanced, performance, or realtime mode;
  - writes explainable status and decision logs;
  - applies guarded EPP, platform profile, and systemd cgroup actions only with
    `--apply`.
- `optctl` Rust MVP:
  - reads status and decision logs;
  - sets mode through the state directory;
  - exposes benchmark and pin placeholders.
- systemd service and tmpfiles config for `optid`.
- Default `optid.service` runs dry-run; `optid-apply.service` is the explicit
  mutating service.
- D-Bus interface contract for the future first-class control API.
- Kernel config fragments:
  - default adaptive kernel;
  - optional PREEMPT_RT kernel;
  - experimental sched_ext fragment.
- UKI-first boot config and systemd-sysupdate descriptors (renamed from
  `adaptive` to `rush-linux` branding; `InstancesMax=3` for rollback retention).
- nftables baseline firewall.
- cgroup and slice accounting defaults.
- Source recipe skeletons for kernel, RT kernel, optid, systemd, desktop, and
  server profiles.
- Edition profiles for desktop, laptop, realtime audio, and server.
- Benchmark manifest covering responsiveness, battery, gaming, realtime audio,
  and server throughput.
- Validation script enforcing required files and future-facing defaults.

- D-Bus server implementation inside `optid` and client integration inside `optctl` (supporting both system bus calls and file-based fallback).
- Rust workspace compilation and test verification on Windows using local Rust toolchain.
- Dynamic system-wide policy loading and parsing (`/config/optid/policy.toml`) with clean fallback to defaults.
- JSON status output option (`--json`) for `optctl status` to support machine-readable telemetry output.
- Custom package and rootfs builder (`tools/rush-builder.py`) using Python standard libraries (including TOML parsing via `tomllib`).
- Package metadata database generation (`repodata.json`) and signature validation stubs (`repodata.json.sig`).
- Extensible rootfs generator populating output rootfs from recipe-resolved dependency trees.
- GPT raw VM disk image compiler using native `systemd-repart` to format ext4 partition and clone rootfs trees without loop mounts or root privileges.

## Not Yet Implemented

- Boot assessment and rollback retention behavior for the validated UKI boot
 path, including simulated bad-kernel rollback.
  - Boot entry manager: `tools/manage-boot-entries.sh` rotates UKIs into
    versioned rollback entries and prunes entries beyond `INSTANCES_MAX` (default 3).
  - Boot assessment service: `packaging/systemd/optid-boot-assess.service`
    runs after `multi-user.target` and writes a boot-good marker via
    `tools/optid-boot-assess`.
  - Boot assessment tool: `tools/optid-boot-assess` supports `mark-good`,
    `check`, `count-failed`, and `reset` commands for boot-good/bad tracking.
  - Rollback integration test: `tools/test-rollback.sh` validates all three
    v0.4 exit criteria (UKI boot, rollback entry retention, bad-kernel rollback).
- Update signing system replaces the mock signature stub with real Ed25519
  signatures:
  - `tools/sign_updates.py` — Python module for generating Ed25519 key pairs
    and signing/verifying `repodata.json` using the `cryptography` library.
  - `tools/sign-updates.sh` — Shell wrapper using OpenSSL as a fallback.
  - `tools/test-sign-updates.sh` — Validates key generation, signing,
    verification, and tamper detection.
  - Test keys are stored in `config/keys/` (private key git-ignored).
  - `rush-builder.py repo-init` now uses real signatures when keys are present.
- Real UKI signing keys, Secure Boot enrollment path, and measured boot policy.
- eBPF probes and overhead budget enforcement.
- GPU, foreground app, video call, fullscreen, and build-system detection.
- Hardware allowlist database for unsafe device power knobs.
- Installer and role selection UI.
- Benchmark harness execution and published results.
- The `Cargo.lock` file is checked in, but it still needs confirmation from
  real Cargo on Linux.
- `optid` sysctl actuation: the per-mode `vm.*` keys (`vm_swappiness`,
  `vm_dirty_*`) defined in `config/optid/policy.toml` are **not yet applied** by
  the daemon. The Rust MVP parses policy but ignores these keys, so the running
  system uses kernel defaults for them. This is deliberate (no aggressive
  swappiness is applied unconditionally; the old static drop-in that did so was
  removed to resolve the ADR 0004 conflict). Tracked follow-up: implement gated
  `vm.*` actuation in `optid` — high `vm.swappiness` must remain conditional on
  ZRAM-backed swap (`high_swappiness_requires_zram`), and every write must go
  through the explainable allowlist per ADR 0009.
- Bootable VM disk image (`disk.raw`) produced by `tools/build-vm-final.sh`.
 Verified 2026-06-08: QEMU direct-kernel boot reaches `multi-user.target`
 with `optid.service` active.
- UEFI UKI VM boot validation through OVMF/systemd-boot is now implemented via
 `tools/validate-uefi-boot.sh` and verified 2026-06-08: OVMF loads
 `EFI/BOOT/BOOTX64.EFI`, systemd-boot selects the Rush Linux entry, the UKI
 loads its embedded initrd, the root filesystem mounts from `/dev/vda2`,
 systemd reaches `multi-user.target`, and `optid.service` starts.
- Rollback entry management (`tools/manage-boot-entries.sh`) rotates UKI
  entries and retains at least 3 rollback entries.
- Boot assessment marker service (`optid-boot-assess.service`) and tool
  (`optid-boot-assess`) for marking boots as good/bad.
- Rollback integration test (`tools/test-rollback.sh`) validates UKI boot,
  rollback entry retention, and bad-kernel recovery.
- Update metadata signing with Ed25519 test keys (`tools/sign_updates.py`,
  `tools/sign-updates.sh`) and signing validation test
  (`tools/test-sign-updates.sh`).

## Known Local Constraints

- `rustc` and `cargo` are installed locally under both Windows host and WSL2 Ubuntu environment.
- `gh` is not installed locally.
- The GitHub repository exists at `https://github.com/Nan0pk/Rush-linux`.

## Acceptance Rule

Any change that modifies behavior, defaults, policy, boot/update flow, kernel
fragments, recipes, service files, or command behavior must update the relevant
documentation in the same change.
