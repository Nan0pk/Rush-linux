# ADR 0010: Realtime Edition Kernel Policy

Status: proposed

> Marked **proposed**; needs human ratification. Addresses review item B4.

## Context

`kernel-policy.md` mandates `PREEMPT_DYNAMIC` as the default kernel. The
realtime-audio edition is a first-class v0.7 edition targeting professional
realtime users, but the docs only say it uses `PREEMPT_RT` "when selected".
PipeWire + RTKit cannot eliminate scheduling jitter under load on a
`PREEMPT_DYNAMIC` kernel, so the two positions conflict: if the realtime edition
does not default to RT it fails to deliver realtime guarantees; if it does, it
appears to contradict the kernel-policy default.

## Decision (proposed)

The kernel default is **per default, not per edition**. `PREEMPT_DYNAMIC` is the
default *for the general-purpose editions* (desktop, laptop, server). The
**realtime-audio edition explicitly overrides** that default and ships the
`linux-adaptive-rt` (`PREEMPT_RT`) kernel as its standard kernel, not an
optional add-on. This is consistent with the existing forbidden-shortcut "do not
make `PREEMPT_RT` the *universal* default" — the realtime edition is a scoped,
opt-in target, not the universal default.

`kernel-policy.md` is amended to state this explicitly: editions may override the
kernel default within documented bounds, and the realtime edition's documented
override is `PREEMPT_RT`.

## Consequences

- `recipes/core/linux-rt.toml` is the realtime edition's default kernel; the
  edition recipe depends on it rather than treating it as optional.
- Realtime guarantees are actually delivered to the edition's target users.
- The "no universal RT default" rule is preserved for all other editions.
- `optid` realtime mode (controlled RT access) layers on top of the RT kernel.
