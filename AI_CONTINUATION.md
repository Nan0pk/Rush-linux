# AI Continuation

This file is for future AI agents or human maintainers continuing the project.
Read it before making changes.

## Mission

Continue building Adaptive Linux: a future-aligned, source-built Linux
distribution centered on `optid`, a fast and explainable runtime optimizer for
responsiveness, battery life, thermals, and resource utilization.

The project goal is serious OS engineering, not random tweak accumulation.

## Forbidden Shortcuts

Do not:

- Replace the distro architecture with a derivative distro script.
- Add X11, PulseAudio, iptables, cgroup v1, SysV init, OpenRC, runit, TLP,
  power-profiles-daemon, TuneD, laptop-mode-tools, pm-utils, or old network
  scripts as defaults.
- Make PREEMPT_RT the universal kernel default.
- Make sched_ext production-critical while its upstream ABI is unstable.
- Add shell scripts that fight `optid` over CPU, power, cgroup, or I/O knobs.
- Add opaque AI/ML tuning before deterministic policy has benchmarks and
  rollback.
- Touch privileged sysfs paths without an allowlist and an explanation path.
- Treat docs as optional cleanup. Docs must stay aligned with code and config.

## Current Status

Implemented:

- Release governance exists with `VERSION`, `RELEASES.md`,
  `docs/versioning.md`, `docs/release-policy.md`,
  `docs/release-checklist.md`, `docs/release-plan-v1.md`, and
  `release/milestones.toml`.
- Rust workspace with `crates/optid` and `crates/optctl`.
- `optid` MVP reads PSI, AC/battery, thermal, and load signals.
- `optid` emits explainable decisions and applies guarded actions only with
  `--apply`.
- `optctl` supports status, explain, mode, trace, and benchmark placeholders
  through the state directory.
- systemd service, D-Bus contract, nftables baseline, kernel fragments, UKI and
  sysupdate descriptors, edition profiles, source recipe skeletons, CI, and
  validation script.

Not implemented yet:

- Real D-Bus server/client integration.
- Rust compilation verification in this Windows workspace.
- Rootfs/package builder.
- Bootable ISO or installer.
- Hardware benchmark harness.
- eBPF probes.
- Signed packages, repo metadata, and real update server.

## Safe Assumptions

- Mainstream x86_64 and ARM64 upstream-supported hardware is the initial target.
- Proprietary firmware may be optional where needed for practical hardware
  support.
- systemd with unified cgroup v2 is the resource-control foundation.
- Wayland, PipeWire, nftables, UKI, PSI, zswap, and cgroup v2 are default
  direction.
- The default kernel is adaptive low-latency with PREEMPT_DYNAMIC; RT is a
  specialist package.
- `optid` is the only default runtime policy owner.

## Repo Layout

```text
crates/optid/             Optimization daemon MVP
crates/optctl/            CLI MVP
config/optid/             Optimizer policy defaults
distro/boot/              UKI and kernel command line defaults
distro/editions/          Install-time role profiles
distro/kernel/            Kernel config fragments
distro/network/           nftables baseline
distro/systemd/           cgroup and slice defaults
distro/sysupdate/         systemd-sysupdate descriptors
packaging/dbus/           D-Bus API contract
packaging/systemd/        optid unit and tmpfiles
recipes/                  Source recipe skeletons
benchmarks/               Benchmark manifest
docs/                     Architecture docs and ADRs
tools/                    Validation and publishing helpers
```

## Commands And Checks

On this Windows workspace:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\validate-repo.ps1
git status --short
```

On Linux with Rust installed:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
./tools/build-rootfs.sh
```

Publishing target:

```text
https://github.com/Nan0pk/Rush-linux
```

## Next Task

Current project version is `0.1.0-alpha.0`. The next milestone is
`0.1.0-alpha.1`, Compile-Clean Core.

First, install or provide a Rust toolchain and make CI pass:

1. Run `cargo fmt`, `cargo test`, and `cargo clippy`.
2. Fix any Rust compile or lint errors without weakening policy.
3. Replace the file-based `optctl` control path with the D-Bus API defined in
   `packaging/dbus/io.adaptive.Optid.xml`.
4. Keep `IMPLEMENTATION_STATUS.md`, `ROADMAP.md`, and the relevant docs updated
   in the same commit.
