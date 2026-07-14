# ADR 0013: Workload Detection And The ML Boundary

Status: proposed

> Marked **proposed**; needs human ratification. Addresses review item B3.

## Context

`non-goals.md` rules out "opaque ML policy before deterministic policy and
benchmarks exist." But the adaptive engine's intended inputs — foreground app,
game, video-call, build-system detection, fullscreen/VRR and GPU-routing state —
are hard to classify deterministically across all apps, vendors, and display
setups. The risk is a false choice at v0.6: ship brittle heuristics, quietly add
ML and violate the non-goal, or drop detection and ship a weaker optimizer than
advertised.

## Decision (proposed)

Resolve the tension by separating *signals* from *policy* and defining what "ML"
means here:

1. **Signals may be probabilistic; policy must be deterministic and
   explainable.** A detector may produce a labelled signal with a confidence
   (e.g. "game: likely, via Steam cgroup + fullscreen"). The *decision* that
   consumes it is a deterministic, inspectable rule, and `optctl explain` always
   states which signal fired and why.
2. **Use authoritative signals first.** Prefer unambiguous OS-level signals over
   guessing: cgroup/scope identity (Steam, Flatpak app IDs), `PipeWire`/portal
   state for active capture (video call), compositor fullscreen/VRR state, and
   GPU routing from the compositor — not screen-scraping or heuristics where an
   authoritative source exists.
3. **The non-goal is a sequencing rule.** Ship deterministic rules + benchmarks
   first. A learned model may later *tune thresholds* only if: it is optional,
   its decisions remain explainable, it can be disabled, and it is gated behind
   benchmarks and rollback. A model that makes opaque, unexplainable policy
   decisions stays out of scope.
4. **Honesty about coverage.** Where a robust signal does not exist, the
   optimizer does nothing special rather than acting on a brittle guess; the
   advertised capability matches what the signals can actually support.

## Consequences

- The adaptive-engine design distinguishes "detectors" (may be confidence-based)
  from "policy" (deterministic, explainable).
- No silent slide into opaque ML; a future learned component has explicit gates.
- Detection scope is bounded by available authoritative signals, which the docs
  must state honestly.
