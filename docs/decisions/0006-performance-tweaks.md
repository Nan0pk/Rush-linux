# ADR 0006: Integrate Latency-Focused Performance Tweaks

Status: accepted (amended — see "Amendment 2026-06: Resolve ADR 0004 conflict")

## Context

Rush Linux aims to maximize responsiveness, prevent write-stalls, and optimize memory utilization during high-stress operations (like source builds). System bottlenecks in standard Linux defaults (like TCP cubic bufferbloat, percentage-based dirty file writes, and double-compression with redundant Zswap/ZRAM configurations) limit overall responsiveness.

## Decision

Incorporate a curated shortlist of best-in-class sysctl performance overrides and dynamic controls:

1. **Low-Latency Network Control:** Set Google BBR and Fair Queueing (`fq`) as defaults.
2. **Prevent Write Stalls:** Replace percentage-based dirty ratios with static bytes limits (`vm.dirty_bytes` / `vm.dirty_background_bytes`) for desktop and laptop profiles.
3. **High-Swappiness ZRAM Tuning:** Use ZRAM swap with `vm.swappiness = 150` to prioritize physical memory for active file buffers and execution.
4. **Compression Conflict Resolution:** Ensure `zswap` is deactivated or disabled dynamically when ZRAM is the active primary swap device to prevent CPU-intensive double compression.

## Consequences

- The `optid` package ships `/usr/lib/sysctl.d/99-rush-network.conf` containing
  **only** the network defaults (BBR + `fq`). See the amendment below.
- Desktop and laptop profiles benefit from smoother file writes and reduced UI lag.
- The default kernel configs retain `CONFIG_ZRAM` and `CONFIG_ZSWAP` support, but runtime policies enforce their mutual exclusion to prevent overhead.

## Amendment 2026-06: Resolve ADR 0004 conflict

The original decision shipped a single static drop-in
(`/usr/lib/sysctl.d/99-adaptive-performance.conf`) that set `vm.dirty_bytes`,
`vm.dirty_background_bytes`, and `vm.swappiness = 150` **unconditionally at
boot**. This directly contradicted ADR 0004 ("optid is the only default owner of
runtime optimization policy") and was incorrect on systems without active ZRAM,
where `vm.swappiness = 150` is harmful. A laptop on battery, a server under
load, and a desktop all received identical aggressive values — defeating the
project's premise.

Resolution:

1. The static drop-in is split. Only **network** defaults that optid does not
   manage (BBR + `fq`) remain static, in `99-rush-network.conf`.
2. The **memory/VM/swap** knobs (`vm.swappiness`, `vm.dirty_bytes`,
   `vm.dirty_background_bytes`) are moved into optid's policy
   (`config/optid/policy.toml`) as **mode-dependent, optid-owned** settings.
   High swappiness is gated on ZRAM being the active swap device.
3. Decision item 3 above ("`vm.swappiness = 150`" applied broadly) is
   **superseded**: 150 is now only a `performance`-mode value and only meaningful
   with ZRAM; battery/realtime use conservative values.

Follow-up (tracked): the optid actuator does not yet write `vm.*` sysctls; it
currently ignores those policy keys. Implementing guarded sysctl actuation
(with an allowlist and `optctl explain` coverage) is required to make these
settings active. Until then, no aggressive swappiness/dirty value is applied at
boot, which is the safe state.
