# 💡 Open Strategic Questions (The Ledger)

**Status:** 🟢 Seed
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
A living ledger of the most critical unanswered questions regarding the architecture and market position of Rush Linux.

### ❓ The "Why" (Motivation)
Prevents critical architectural gaps from being ignored until the release phase.

### 🛠️ The Current Ledger
- **Q1: Intent Architecture:** Universal Wayland bridge vs. compositor-specific adapters?
- **Q2: sched_ext:** Should v0.6/0.7 include a spike to prototype BPF scheduler orchestration?
- **Q3: Scope:** Should GPU/PCIe/USB power management be in scope for v1.0?
- **Q4: Position:** End-user desktop vs. Developer/Builder OS?
- **Q5: Hardware Profiles:** Safe internal allowlists vs. crowdsourced community profiles?
- **Q6: Sysctl Gaps:** When to implement gated `vm.swappiness` and `vm.dirty_*` actuation?

### ⚠️ Trade-offs & Risks
- **Analysis Paralysis:** Spending too much time in the "question phase" instead of the "implementation phase."

### ⏳ Why not now?
These are intended to be resolved incrementally during the v0.5 to v1.0 transition.
