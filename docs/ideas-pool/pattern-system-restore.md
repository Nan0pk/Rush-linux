# 💡 Single-Command System Restoration

**Status:** 🔴 Proposal
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Add an `optctl reset` command that restores all mutated system knobs to their known-safe boot-time defaults in one atomic operation.

### ❓ The "Why" (Motivation)
User trust. If a user experiments with a bad policy and the system becomes sluggish, they need a "Panic Button" to return to a stable state without rebooting.

### 🛠️ Potential Implementation
1. Capture a snapshot of all tuned parameters at boot.
2. `optctl reset` reads this snapshot and writes the values back to sysfs/cgroups.
3. Clear all transient `optid` state.

### ⚠️ Trade-offs & Risks
- **Storage:** Requires maintaining a snapshot of the system state on disk.

### ⏳ Why not now?
Essential for user trust; should be a v1.0 blocker.
