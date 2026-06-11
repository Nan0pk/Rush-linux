# Adaptive Engine

`optid` is the central feature of Rush Linux. It is a privileged daemon that
observes workload and hardware state, then applies guarded policy changes to
improve responsiveness, battery behavior, thermals, and resource utilization.

## Current MVP

The current Rust implementation:

- reads PSI from `/proc/pressure/{cpu,memory,io}`;
- reads AC/battery state from `/sys/class/power_supply`;
- reads thermal state from `/sys/class/thermal`;
- reads load average from `/proc/loadavg`;
- detects ZRAM-backed swap from `/proc/swaps`;
- chooses battery, balanced, performance, or realtime mode;
- writes status and decision logs under `/run/optid`;
- applies guarded actions only when `--apply` is passed;
- actuates per-mode `vm.swappiness`, `vm.dirty_background_bytes`, and
  `vm.dirty_bytes` through the write allowlist. High swappiness (>60) is
  clamped to 60 unless ZRAM swap is active
  (`memory.high_swappiness_requires_zram` in `policy.toml`). Before the first
  write of each sysctl the original value is backed up under
  `/run/optid/original_vm_*` and the intended value recorded under
  `/run/optid/intended_vm_*`; the daemon restores the originals on startup and
  on clean exit, appending a `revert` line to `actions.log`. A failed sysctl
  write or revert is logged and skipped (the backup is kept for the next
  attempt), never fatal.

`optctl` communicates with `optid` via D-Bus as defined in `packaging/dbus/io.rushlinux.Optid.xml`, with automatic fallback to files in the state directory if D-Bus is offline.

The packaged default `optid.service` runs in dry-run mode. Mutating policy is
split into `optid-apply.service` so early releases cannot silently change CPU,
platform, or cgroup settings without an explicit service choice.

## Policy Ownership

`optid` is the only default owner of runtime optimization knobs. This avoids
policy fights between power daemons, desktop widgets, shell scripts, and service
drop-ins.

Conflicting services are declared in `packaging/systemd/optid.service` and
`config/optid/policy.toml`.

## Inputs

Accepted input classes:

- PSI for CPU, memory, and I/O pressure.
- Per-cgroup pressure and systemd unit/session metadata.
- AC/battery state, battery percentage, lid/dock state, and suspend signals.
- Thermal zones and platform profile availability.
- CPUFreq, CPPC, Intel/AMD P-state, EPP, turbo state, and CPU topology.
- GPU power state, display mode, fullscreen/VRR state, and dGPU routing.
- Storage class, I/O latency, swap, zswap, and zram activity.
- Foreground app, game, compiler/build, video call, and realtime audio state.
- Optional eBPF probes with explicit overhead limits.

## Actions

Accepted action classes:

- systemd runtime cgroup properties such as CPU weight, I/O weight, memory
  protection, and OOM policy.
- CPU energy performance preference and platform profile changes.
- Background throttling during pressure, heat, video calls, or battery use.
- VM sysctls (`vm.swappiness`, `vm.dirty_background_bytes`, `vm.dirty_bytes`)
  per mode, with high swappiness gated on detected ZRAM swap; further
  zswap/zram/swap policy after memory pressure support is implemented.
- GPU, PCIe, USB, NVMe, Wi-Fi, and display power policy only through hardware
  allowlists.

## Guardrails

- Dry-run remains available.
- Every action must have a reason visible through `optctl explain`.
- Hysteresis and cooldowns are required before aggressive policy expansion.
- Unsafe sysfs writes require explicit allowlisting.
- Hardware-specific policy must degrade safely when sensors or firmware knobs
  are missing.
