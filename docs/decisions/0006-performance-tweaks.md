# ADR 0006: Integrate Latency-Focused Performance Tweaks

Status: accepted

## Context

Adaptive Linux aims to maximize responsiveness, prevent write-stalls, and optimize memory utilization during high-stress operations (like source builds). System bottlenecks in standard Linux defaults (like TCP cubic bufferbloat, percentage-based dirty file writes, and double-compression with redundant Zswap/ZRAM configurations) limit overall responsiveness.

## Decision

Incorporate a curated shortlist of best-in-class sysctl performance overrides and dynamic controls:

1. **Low-Latency Network Control:** Set Google BBR and Fair Queueing (`fq`) as defaults.
2. **Prevent Write Stalls:** Replace percentage-based dirty ratios with static bytes limits (`vm.dirty_bytes` / `vm.dirty_background_bytes`) for desktop and laptop profiles.
3. **High-Swappiness ZRAM Tuning:** Use ZRAM swap with `vm.swappiness = 150` to prioritize physical memory for active file buffers and execution.
4. **Compression Conflict Resolution:** Ensure `zswap` is deactivated or disabled dynamically when ZRAM is the active primary swap device to prevent CPU-intensive double compression.

## Consequences

- The `optid` package now includes and installs `/usr/lib/sysctl.d/99-adaptive-performance.conf` to configure network, VM, and swappiness settings.
- Desktop and laptop profiles benefit from smoother file writes and reduced UI lag.
- The default kernel configs retain `CONFIG_ZRAM` and `CONFIG_ZSWAP` support, but runtime policies enforce their mutual exclusion to prevent overhead.
