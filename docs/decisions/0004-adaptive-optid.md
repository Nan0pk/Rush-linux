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

