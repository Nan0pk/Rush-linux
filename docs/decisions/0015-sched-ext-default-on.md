# ADR 0015: sched_ext as Default-On Centerpiece

Status: proposed

> This ADR proposes enabling sched_ext by default on desktop/laptop editions with EEVDF fallback, amending the kernel default scope of ADR 0010 and the sched_ext warning in non-goals.md.

## Context

Previously, `non-goals.md` declared that depending on sched_ext for production behavior before upstream stability is sufficient was a non-goal. Furthermore, ADR 0010 established kernel default policies focusing on PREEMPT_DYNAMIC versus PREEMPT_RT. 

To achieve responsiveness comparable to macOS, we need scheduler-level capabilities that can dynamically prioritize foreground session latency over background tasks. Heuristic or EPP/cgroup weight adjustments alone are insufficient for tail-latency clipping under heavy loads. The `sched_ext` extensible scheduler framework allows us to load custom schedulers (like `scx_bpfland` or `scx_lavd`) dynamically to meet this requirement.

## Decision (proposed)

1. **Default-on for Desktop/Laptop:** Enable `sched_ext` by default on desktop and laptop editions.
2. **Dynamic loading via optid:** `optid` will manage the lifecycle of the scheduler via `scx_loader`.
3. **EEVDF Fallback:** Implement verified automatic fallback to the standard EEVDF scheduler as the safety net in case of scheduler eviction or load errors.
4. **Safety Limits:** Poll `/sys/kernel/sched_ext/state`. On scheduler eviction, log a decision record and enforce a cooldown period (e.g. 10 minutes) before attempting reload. Pin standard EEVDF for the remainder of the session after 3 consecutive evictions.
5. **No RT Stacking:** Never stack `sched_ext` on the `PREEMPT_RT` (realtime-audio) edition, preserving the core kernel scheduling guarantees for pro-audio workloads.

## Consequences

- The `non-goals.md` statement on sched_ext is updated to reference this ADR.
- `optid` is extended to support `scx_loader` integration.
- Desktop and laptop responsiveness is measurably improved under mixed loads.
