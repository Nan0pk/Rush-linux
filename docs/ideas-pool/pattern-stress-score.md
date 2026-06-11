# 💡 Continuous Stress Score vs. Discrete Thresholds

**Status:** 🟡 Sketch
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Replace discrete thresholds (`if load > X then switch to performance`) with a continuous stress score (scalar between 0 and 1) that collapses multi-dimensional inputs (CPU, I/O, memory, thermal, battery) into a single value.

### ❓ The "Why" (Motivation)
Discrete thresholds cause "mode-flipping" and require complex hysteresis logic. A continuous score allows for smooth actuation:
- **Low score:** Relax constraints, maximize battery savings.
- **Rising score:** Progressively boost foreground weight, tighten EPP.
- **High score:** Maximum performance, aggressive background throttling.

### 🛠️ Potential Implementation
The score would drive the control-loop cadence: fast when stressed, slow when idle. To preserve explainability, the internal scalar is mapped to named modes (`battery`, `balanced`, `performance`) for `optctl explain` output.

### ⚠️ Trade-offs & Risks
- **Explainability:** A scalar is inherently less intuitive than a "rule" (e.g., "Load > 80%").
- **Tuning:** Finding the correct weights for the scalar collapse requires extensive empirical testing.

### ⏳ Why not now?
Needs a mathematical model for the stress score and validation that it doesn't introduce oscillation.
