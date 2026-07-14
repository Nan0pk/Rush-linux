# ADR 0005: Avoid Obsolete Defaults

Status: accepted

## Context

This is a long-term distro project. Choosing components that are already
obsolete or being replaced would create avoidable migration work and poor
technical direction.

## Decision

Do not use obsolete or near-obsolete modules as defaults when a mature modern
replacement exists.

Default direction:

- systemd with cgroup v2, not legacy init or cgroup v1.
- Wayland, not X11.
- PipeWire/WirePlumber, not PulseAudio as the default audio server.
- nftables, not iptables-family tools as primary firewalling.
- UKI-first boot, not legacy-only boot layout.
- PREEMPT_DYNAMIC default kernel, not universal PREEMPT_RT.

## Consequences

- Compatibility can exist later, but defaults stay modern.
- Validation checks must reject accidental legacy defaults.
- Docs must explain any exception before code adds it.

