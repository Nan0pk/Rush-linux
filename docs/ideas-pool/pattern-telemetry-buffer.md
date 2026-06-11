# 💡 Telemetry Ring Buffer for Benchmark Correlation

**Status:** 🟡 Sketch
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Implement an optional, privacy-safe telemetry ring buffer that records high-frequency snapshots of sensors and decisions for post-benchmark analysis.

### ❓ The "Why" (Motivation)
Standard logs are too coarse. A high-frequency telemetry stream allows us to correlate exact sensor spikes with exact `optid` decisions, enabling precise policy tuning.

### 🛠️ Potential Implementation
Write periodic snapshots (metric labels + values) to a compressed binary ring buffer. Data is only exposed via `optctl` for local analysis.

### ⚠️ Trade-offs & Risks
- **Privacy:** High-frequency logging can be invasive.
- **Overhead:** Even a ring buffer consumes CPU/IO cycles.

### ⏳ Why not now?
Requires a strict privacy policy and an "opt-in" consent model before implementation.
