# Kernel Policy

Rush Linux uses upstream kernel capabilities first. Kernel policy must
support low latency, adaptive control, power management, observability, and
rollback without forcing specialist realtime behavior on every system.

## Default Kernel

The default kernel is represented by
`distro/kernel/default-adaptive.config`.

Required direction:

- `PREEMPT_DYNAMIC` for runtime preemption flexibility.
- PSI enabled for pressure-aware policy.
- cgroup v2 controllers and BPF integration.
- uclamp support for future scheduling hints.
- zswap and zram support for memory pressure control (run as mutually exclusive to avoid double compression overhead).
- Multi-Gen LRU (MGLRU) enabled by default for improved reclaim selection under memory pressure.
- DAMON support for future memory policy.
- BFQ and mq-deadline support for device-appropriate I/O policy.
- modern CPUFreq, Intel P-state, AMD P-state, and CPPC support.
- EFI stub support for UKI generation.
- Landlock/SELinux path for security policy.

## Realtime Kernel

`distro/kernel/realtime.config` defines an optional PREEMPT_RT package.

PREEMPT_RT is not the universal default because it can add overhead, expose
driver issues, and reduce throughput for workloads that do not need bounded
latency.

## Experimental Scheduler Work

`distro/kernel/experimental-sched-ext.config` is experimental only. Production
profiles must not depend on sched_ext while upstream documents ABI instability.

## Acceptance Criteria

Kernel policy changes must update:

- this document;
- related recipes under `recipes/core/`;
- relevant ADRs if the default direction changes;
- benchmark expectations when latency, throughput, or power behavior changes.

