# 💡 Proactive Intent Detection for optid

**Status:** 🟡 Sketch
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Shift `optid` from a reactive engine (responding to pressure) to a proactive one that detects user intent and aligns resources *ahead* of demand.

### ❓ The "Why" (Motivation)
Current reactive loops always "play catch-up." By detecting signals like active PipeWire streams (calls) or Wayland foreground apps, the OS can pre-warm the CPU and optimize latency before the user perceives a lag.

### 🛠️ Potential Implementation
- **Phase 1 (Stable Signals):** Integrate PipeWire metadata, systemd-logind (idle/locked), and eBPF `execve` tracing.
- **Phase 2 (Foreground Awareness):** Create a compositor abstraction layer (KWin/Mutter/Sway) to detect the active window.
- **Phase 3 (Predictive):** Use stateful learning to correlate signal history with load spikes.

### ⚠️ Trade-offs & Risks
- **Complexity:** Building a compositor-agnostic intent layer is a significant engineering effort.
- **Privacy:** Tracking foreground apps and execve calls requires a strong security and transparency model.

### ⏳ Why not now?
Requires a stable abstraction layer for Wayland compositors and a decision on whether this lives in `optid` or a separate `optid-intentd` helper.
