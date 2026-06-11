# 💡 Protected Service Exclusion List

**Status:** 🟢 Seed
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Implement a `protected_units` list in `policy.toml` that exempts critical system services from any form of deprioritization or throttling.

### ❓ The "Why" (Motivation)
Under extreme pressure, `optid` might accidentally throttle a critical service (like `sshd` or `systemd-journald`), potentially locking the user out of the system or losing critical logs.

### 🛠️ Potential Implementation
A simple allowlist in `policy.toml` that the policy engine checks before applying any `cpu.weight` or `io.weight` reduction.

### ⚠️ Trade-offs & Risks
- **Resource Leak:** Too many protected services can starve the foreground task.

### ⏳ Why not now?
Can be implemented as a simple safety feature in v0.6.
