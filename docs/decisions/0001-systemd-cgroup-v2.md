# ADR 0001: Use systemd With Unified cgroup v2

Status: accepted

## Context

Rush Linux needs hierarchical CPU, memory, I/O, task, and OOM control for
applications, services, user sessions, and background work. The optimizer must
be able to observe and adjust policy without inventing a separate service model.

## Decision

Use systemd as the init and service manager. Require unified cgroup v2 as the
default resource-control hierarchy.

## Consequences

- `optid` can use systemd runtime properties for cgroup policy.
- Desktop sessions, services, and server workloads share one resource model.
- cgroup v1 and dual-init support are out of scope for defaults.
- Service files and docs must keep systemd as the assumed foundation.

