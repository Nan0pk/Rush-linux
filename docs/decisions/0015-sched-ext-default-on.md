# ADR 0015: sched_ext Default-On With EEVDF Fallback

Status: proposed

> Decided by the project owner in the 2026-06-10 strategy session (recorded in
> `docs/plans/handover-2026-06-10-ecosystem-and-benchmarks.md` §1) but marked
> **proposed** pending formal ratification: per the ADR lifecycle, only a human
> maintainer may set `accepted` with a `Ratified-by:` line.

## Context

Rush's goal is macOS-comparable felt responsiveness. The 2026-06-10 host
benchmark campaign (HP Victus, three replications; handover doc §5.3)
established empirically that the current optid knobs — EPP and cgroup
weights — are power levers, not felt-latency levers: no mode moved p95 wakeup
latency under load. Scheduler-level work is therefore the only credible path
to the responsiveness goal. sched_ext (mainline since kernel 6.12) allows
shipping purpose-built schedulers (`scx_bpfland`, `scx_lavd` — both designed
for interactive/desktop workloads) as BPF programs with a kernel-guaranteed
safety property: if the BPF scheduler misbehaves or is unloaded, the kernel
falls back to the default scheduler (EEVDF) automatically.

## Decision (proposed)

1. **sched_ext ships default-on for the desktop and laptop editions**, running
   `scx_bpfland` or `scx_lavd` managed through `scx_loader` (its D-Bus API),
   with EEVDF as the verified automatic fallback. Server edition stays on
   EEVDF by default.
2. **optid is the sole policy driver** (extends ADR 0004): optid talks to
   `scx_loader`; nothing else starts, stops, or switches scx schedulers. On
   mode change optid switches scx *profiles*; switching *schedulers* happens
   only on edition or explicit-mode boundaries.
3. **Eviction handling:** optid polls `/sys/kernel/sched_ext/state`. On
   eviction it logs a decision record, waits a 10-minute cooldown before
   reloading, and after 3 evictions in one boot pins EEVDF for the session
   (visible in `optctl explain`).
4. **The realtime-audio edition never stacks scx on PREEMPT_RT** (amends the
   scope wording of ADR 0010: that edition's scheduling story is PREEMPT_RT
   itself, and optid's realtime mode must not load a scx scheduler there).
5. **Gate before default-on ships:** a soak test plus
   `tools/test-scx-fallback.sh` (to be written) demonstrating scheduler load,
   forced failure, automatic EEVDF fallback, and optid's eviction record —
   transcripts required. sched_ext consumes its milestone's entire
   novel-risk budget (admission rule 4): no other novel-risk component lands
   in the same milestone.

## Consequences

- The kernel config must enable `CONFIG_SCHED_CLASS_EXT` (and BPF
  prerequisites) in `distro/kernel/default-adaptive.config`; the Arch base
  (ADR 0014) ships kernels new enough on cadence.
- New external dependencies: the scx suite and `scx_loader` (run as external
  processes; no license interaction with this Apache-2.0 repo).
- A failing scx scheduler degrades to stock-Linux behavior, never worse —
  this is the safety property that makes default-on acceptable.
- Responsiveness claims move from "tuning" to "scheduling": benchmark
  methodology (ADR 0011) gains scheduler-latency scenarios; the existing
  wakeup-latency harness already provides the baseline.
- `docs/kernel-policy.md` gains the sched_ext requirement; ADR 0010's
  realtime-edition policy is unchanged except the explicit no-scx-on-RT rule.

## Alternatives considered

- **Keep EEVDF everywhere, tune via cgroups/EPP.** Rejected: three
  replications showed those knobs do not move felt latency (handover §5.3).
- **Run system76-scheduler or similar daemons.** Rejected: violates "one
  owner per knob" (ADR 0004); GPL code cannot be incorporated into this
  repo — only its heuristics, independently re-implemented.
- **Patch a custom CFS/EEVDF.** Rejected: a kernel fork is unmaintainable at
  this project's size and loses the automatic-fallback safety property.
- **scx opt-in instead of default-on.** Rejected by owner decision: the
  scheduler IS the responsiveness centerpiece; opt-in would make the
  flagship feature invisible. The EEVDF fallback and the eviction policy
  bound the risk.
