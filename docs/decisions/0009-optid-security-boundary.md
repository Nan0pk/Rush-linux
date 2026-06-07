# ADR 0009: optid Security Boundary And Threat Model

Status: proposed

> Marked **proposed**; needs human ratification. Addresses review item C3.

## Context

`optid` is a privileged daemon: it writes to sysfs, sets cgroup properties
across all user sessions, and exposes a D-Bus API. `SECURITY.md` lists sensitive
areas but contains no threat model. The D-Bus API is already implemented, so its
authentication surface is already being established; changing it post-v1 is a
breaking change, and tightening the systemd sandbox after deployment is painful.
These are cheapest to get right now.

## Threat model

Assets: integrity of system tunables (CPU EPP, platform profile, cgroup
weights, future `vm.*`), availability/responsiveness, and the explainability
guarantee.

Actors / threats:

1. **Unprivileged local user** calling D-Bus to force a mode (e.g. `performance`
   to drain a laptop battery, or to deny service to other users via cgroup
   weights).
2. **Adversarial inputs**: a hostile `/proc/pressure/*` or sysfs value (e.g. a
   FUSE mount returning crafted PSI values) steering decisions.
3. **Compromised `optid`**: code-exec in the daemon; blast radius is whatever it
   may write.
4. **Supply chain / config tampering** of `policy.toml`.

## Decision (proposed)

1. **D-Bus authorization via polkit.** Read-only methods (`Status`, `Explain`,
   `Mode` getter) are allowed for any local active session. State-changing
   methods (`SetMode`, `PinApplication`) require a polkit action; default policy:
   allowed for active local sessions with admin auth, denied for remote/inactive.
   No method is callable by arbitrary remote peers.
2. **Strict write allowlist (already partially present).** `optid` may only
   write an explicit allowlist of sysfs/proc paths
   (`/sys/devices/system/cpu/**/energy_performance_preference`,
   `/sys/firmware/acpi/platform_profile`, and — when sysctl actuation lands — a
   named set of `/proc/sys/vm/*`). Any write outside the allowlist is refused
   and logged. This bounds the blast radius of a compromise.
3. **Input distrust.** Treat `/proc` and sysfs reads as untrusted: clamp/range-
   check values, and never let a single adversarial reading cause an unbounded
   action. Decisions remain explainable via `optctl explain`.
4. **Sandbox hardening of the unit.** Audit and tighten `optid.service`:
   add `SystemCallFilter=@system-service` (deny `@privileged`/`@obsolete`),
   `ProtectKernelModules=yes`, `ProtectControlGroups=` set appropriately for the
   cgroup writes it needs, `RestrictNamespaces=yes`, `LockPersonality=yes`,
   `MemoryDenyWriteExecute=yes`, `RestrictRealtime=yes` (except realtime mode),
   and keep `CapabilityBoundingSet` to the minimum (`CAP_SYS_NICE`,
   `CAP_SYS_RESOURCE`; drop `CAP_SYS_ADMIN` if achievable).
5. **Disclosure process.** Replace "report to repo owner" with a documented
   process: a security contact, a GPG key, and a target initial-response SLA.
   Track advisories.

## Consequences

- Adds a polkit policy file to the `optid` package and a `SECURITY.md` rewrite.
- The systemd unit gains a hardening block; each directive must be validated
  against the daemon's real syscall/cgroup needs (T2/T5 testing).
- The allowlist becomes the single source of truth for "what optid can touch".

## Follow-ups

- Implement polkit checks in the D-Bus methods.
- Add the hardened unit directives and validate under test.
- Rewrite `SECURITY.md` with contact, key, SLA, and this threat model.
