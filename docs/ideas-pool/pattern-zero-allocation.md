# 💡 Zero-Allocation Guarantee in the Control Loop

**Status:** 🔴 Proposal
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Ensure that the `optid` control loop (Read Sensors $ightarrow$ Compute Score $ightarrow$ Queue Adjustments) performs zero heap allocations during the hot path.

### ❓ The "Why" (Motivation)
Heap allocations introduce latency jitter and potential OOM crashes under extreme memory pressure. For a daemon promising low latency, the engine itself must not introduce its own spikes.

### 🛠️ Potential Implementation
- Use fixed-size arrays instead of `Vec`.
- Implement ring buffers for telemetry.
- Use lock-free queues with pre-allocated capacity.
- Pre-allocate all necessary buffers at startup.

### ⚠️ Trade-offs & Risks
- **Flexibility:** Fixed-size buffers limit the number of tracked sensors or policies.
- **Development Effort:** Requires a more disciplined approach to Rust memory management.

### ⏳ Why not now?
This is a safety and quality guarantee that should be a benchmark gate for v0.8.
