# ADR 0004: Make optid The Adaptive Policy Owner

Status: accepted

## Context

The distro's main feature is runtime adaptation. Multiple daemons or scripts
mutating the same CPU, power, cgroup, I/O, and device knobs would create
oscillation and unexplainable behavior.

## Decision

Make `optid` the only default runtime optimization policy owner. Other
components may provide inputs, compatibility APIs, or explicit user intent.

## Consequences

- `optid` conflicts with TLP, power-profiles-daemon, and TuneD as active
  default services.
- Every action must be explainable through `optctl explain`.
- Privileged writes require guardrails and allowlists.
- Compatibility APIs must route intent to `optid`, not bypass it.

## Boundary clarification (2026-06)

"Runtime optimization policy" means knobs whose correct value depends on
workload, hardware, power source, or mode — CPU EPP, platform profile, cgroup
CPU/IO/memory weights, and **memory/VM/swap tuning** (`vm.swappiness`,
`vm.dirty_*`). These are owned by optid and must not be set by static drop-ins.

Static system defaults that optid does **not** adapt per-mode (for example the
network congestion-control / qdisc choice in `99-rush-network.conf`) are not
"runtime optimization policy" and may be shipped statically. This boundary was
established when resolving the ADR 0006 conflict; see ADR 0006's amendment.
