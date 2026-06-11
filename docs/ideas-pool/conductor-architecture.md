# 💡 The Conductor Architecture: optid as Orchestrator

**Status:** 🔴 Proposal
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Instead of `optid` being a tool that writes to sysfs, it becomes the **single owner of all policy decisions**—the "Conductor" of a larger optimization stack.

### ❓ The "Why" (Motivation)
Upstream distros (Fedora/systemd) are adding adaptive controls. To remain defensible, `optid` must move from being a "mechanism" to being the "brain" that orchestrates other mechanisms (like `sched_ext` or systemd slices).

### 🛠️ Potential Implementation
Structure the stack as:
1. **Kernel Layer:** EPP, P-state, PSI, `sched_ext`.
2. **Systemd Layer:** cgroup weights, OOM policy.
3. **eBPF Layer:** Latency histograms, syscall tracing.
4. **optid Layer:** Intent fusion, decision logic, explainability, and rollback.

### ⚠️ Trade-offs & Risks
- **Dependence:** Shifts the focus from "writing a daemon" to "integrating multiple complex subsystems."
- **Stability:** Relying on BPF schedulers (`sched_ext`) introduces ABI instability risks.

### ⏳ Why not now?
Needs validation of `sched_ext` orchestration by v0.7 to ensure the architecture isn't outpaced by the kernel.
