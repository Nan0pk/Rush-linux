# Project Brief

Adaptive Linux is a long-term operating system project for a source-built Linux
distribution whose main product feature is fast, preemptive, automatic runtime
optimization across mainstream hardware and workloads.

The project is not a theme, tweak set, or derivative distro script. It is an
architecture-first distro effort with a native optimizer, `optid`, as a core
system service.

## Mission

Build a modern Linux distribution that:

- Maximizes foreground responsiveness under mixed load.
- Preserves or improves battery life on laptops.
- Uses CPU, memory, storage, GPU, thermal headroom, and background services
  intelligently.
- Dynamically adjusts in real time instead of relying on fixed power profiles.
- Keeps every optimization explainable, reversible, and benchmarked.
- Avoids obsolete defaults and aligns with upstream Linux direction.

## Current Product Shape

The first implementation slice is a GitHub-ready scaffold:

- Rust workspace for `optid` and `optctl`.
- Kernel config fragments for adaptive and realtime kernels.
- cgroup v2, PSI, UKI, nftables, Wayland, PipeWire, and systemd defaults.
- Source recipe skeletons for future package builds.
- Edition profiles for desktop, laptop, server, and realtime audio.
- Documentation and ADRs that define accepted architecture.

This is not yet a bootable distribution. The next milestone is to make the
Rust workspace compile in CI, then build a minimal Linux rootfs from recipes.

## Success Criteria

The distro succeeds only if it can prove:

- Better foreground latency than mainstream distro defaults under mixed load.
- Competitive or better laptop battery behavior.
- No unacceptable throughput loss on workstation and server workloads.
- Safe rollback for bad kernels, updates, and optimizer policy changes.
- Clear documentation that matches the actual code and config.

## Engineering Principles

- Prefer upstream kernel and userspace features over permanent downstream
  patches.
- Avoid legacy defaults unless there is no modern replacement that works.
- Make one component own runtime optimization policy: `optid`.
- Add hardware-specific policies through allowlists and measured data.
- Treat documentation, tests, and validation as part of implementation.

