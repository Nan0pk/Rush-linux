# 💡 Batched Actuation with Adaptive Deadband

**Status:** 🟡 Sketch
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Implement a batched actuation system that queues all computed adjustments and applies them atomically, using an adaptive deadband to filter out noise.

### ❓ The "Why" (Motivation)
Individual adjustments cause unnecessary kernel/userspace round-trips and jitter. A deadband prevents "thrashing" around a setpoint:
- **High stress:** Apply only changes >1% to avoid noise.
- **Low stress:** Apply changes >10% to avoid oscillation.

### 🛠️ Potential Implementation
Queue adjustments in a temporary buffer $ightarrow$ Filter through the adaptive deadband $ightarrow$ Apply as a single atomic batch of sysfs/cgroup writes.

### ⚠️ Trade-offs & Risks
- **Latency:** Batching introduces a tiny delay between computation and actuation.
- **Complexity:** Managing a queue and a dynamic threshold increases engine complexity.

### ⏳ Why not now?
Requires benchmarking to determine the optimal deadband thresholds for different hardware.
