# Non-Goals

This project needs explicit non-goals to avoid becoming a pile of unrelated OS
ideas.

## Not Goals

- Rewriting Linux, systemd, PipeWire, NetworkManager, nftables, Mesa, or the
  package ecosystem from scratch.
- Shipping a derivative distro that only applies post-install tweaks.
- Making PREEMPT_RT the default for every machine.
- Depending on sched_ext for production behavior before the Northstar evidence
  gate and explicit owner approval. ADR 0015 remains proposed and does not
  authorize a default-on production scheduler.
- Running multiple competing power/performance daemons by default.
- Using opaque ML policy before deterministic policy and benchmarks exist.
  (This is a sequencing rule, not a permanent ban — see ADR 0013 for how
  app/game/call detection is handled within it.)
- Optimizing for unsupported proprietary hardware at the expense of system
  stability.
- Maximizing synthetic benchmark scores while hurting foreground behavior,
  thermals, battery, or explainability.
- Supporting dual init systems.
- Preserving obsolete defaults for familiarity when modern replacements are
  mature enough.

## Compatibility Is Not Default

Compatibility packages may exist later for legacy apps or workflows, but they
must not define the default architecture.
