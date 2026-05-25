# Implementation Status

Last updated: 2026-05-25

## Overall State

The repository is an early implementation scaffold, not a bootable distro yet.
It defines the architecture, accepted defaults, Rust optimizer MVP, config
fragments, packaging skeletons, CI, and documentation layer needed for the next
engineering milestones.

## Implemented

- Current project version recorded in `VERSION` as `0.1.0-alpha.0`.
- Release plan, versioning rules, release policy, release checklist, and
  machine-readable milestone/test-tier manifests.
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
- UKI-first boot config and systemd-sysupdate descriptors.
- nftables baseline firewall.
- cgroup and slice accounting defaults.
- Source recipe skeletons for kernel, RT kernel, optid, systemd, desktop, and
  server profiles.
- Edition profiles for desktop, laptop, realtime audio, and server.
- Benchmark manifest covering responsiveness, battery, gaming, realtime audio,
  and server throughput.
- Validation script enforcing required files and future-facing defaults.

## Not Yet Implemented

- Bootable root filesystem from source recipes.
- Real binary package repository and metadata signing.
- Real UKI signing keys, Secure Boot enrollment path, and measured boot policy.
- D-Bus server implementation inside `optid`.
- D-Bus client implementation inside `optctl`.
- eBPF probes and overhead budget enforcement.
- GPU, foreground app, video call, fullscreen, and build-system detection.
- Hardware allowlist database for unsafe device power knobs.
- Installer and role selection UI.
- Benchmark harness execution and published results.
- Rust CI results, because this local Windows workspace has no Rust toolchain.
- The `Cargo.lock` file is checked in, but it still needs confirmation from
  real Cargo on Linux.

## Known Local Constraints

- `rustc` and `cargo` are not installed locally.
- `gh` is not installed locally.
- The GitHub repository exists at `https://github.com/Nan0pk/Rush-linux`.

## Acceptance Rule

Any change that modifies behavior, defaults, policy, boot/update flow, kernel
fragments, recipes, service files, or command behavior must update the relevant
documentation in the same change.
