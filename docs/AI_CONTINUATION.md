# AI Continuation — Agent Handoff Document

This file is the canonical orientation and operational runbook for future AI agents or human maintainers continuing the development of Rush Linux. Read it thoroughly before initiating any codebase changes.

---

## 🌌 Mission

Continue building **Rush Linux**: a modern, verifiable, source-built Linux distribution architecture designed to deliver Apple-class adaptive power efficiency and responsiveness on x86_64 laptops.

Rather than relying on static, competing user-space scripts (e.g., TLP, TuneD, power-profiles-daemon), Rush Linux establishes a crystal-clear, single-owner optimization boundary owned entirely by **`optid`**—a highly explainable, multi-sensor pure-Rust workload orchestrator.

---

## Forbidden Shortcuts

To preserve the uncompromising engineering standards of Rush Linux, you must strictly adhere to the following rules:
- **No Derivative Slop:** Do not replace our declarative image composition plane with custom procedural ISO build scripts.
- **Reject Legacy Technology Base:** Do not introduce X11, PulseAudio, iptables, cgroup v1 baselines, SysV init, OpenRC, runit, or conflicting power daemons (TLP, power-profiles-daemon) as defaults.
- **No Speculative claims (The Evidence Rule):** Any checkmark (✅) or claim of correctness/performance in documentation or milestone ledgers must be accompanied by an authentic command transcript. No claim substitutes for an empirical receipt.
- **Do not bypass the Allowlist:** No sysfs or cgroup writes may occur outside the structural boundaries established in `guarded_write`.
- **Maintain 100% Documentation Synchronization:** Architecture documents, docmap entries, and ADRs are non-negotiable acceptance criteria. Every code or config commit must concurrently update all dependent documents.

---

## 🔄 Session Lifecycle

Every agentic or human work session must follow this turnkey lifecycle:

1. **Start Session:**
   ```bash
   bash tools/start-work.sh "what you are about to implement"
   ```
2. **Execute Work:** Implement code, stage builds, and run our comprehensive verification suite (`cargo test --workspace`).
3. **Verify Documentation Consistency:**
   ```bash
   python3 tools/validate-doc-sync.py
   ```
4. **Finish Session:**
   ```bash
   bash tools/finish-work.sh "highly structured commit message"
   ```

*(Note: If you must detach from a session before completion, create a `DIRTY_STATE.md` recording exactly what was completed and what blocker remains so the next agent can resume seamlessly).*

---

## 📊 Current State of the Architecture

Rush Linux is transitioning from completed Alpha (`v0.4.0-alpha.1`, all exit criteria verified) into its most consequential R&D milestone: **v0.5.0-beta.1 Minimal Installable System** via mkosi/Arch pivot (per ratified **ADR 0014**).

### 🏆 Implemented and Fully Hardened
- **Semantic Issue Boundary (ADR 0016):** Overarching multi-year Epics (Tracks A–D) and specification tracks (`WP-N1`–`WP-N9`, `WP-B1`) have been officially migrated out of open GitHub issues and consolidated into our markdown canon (`docs/SPEC-northstar.md` and `ROADMAP.md`). The GitHub repository presents exactly **1 Open Issue** (`good first issue` #3: *"Split optid into modules"*).
- **Core Optimization Crate (`crates/optid`):** 
  - Pure pure functional workload classifier resolving 5 distinct SPEC §1 workload classes (`idle`, `light`, `interactive`, `latency-critical`, `throughput`) with hysteresis and IPC D-Bus pinning (`optctl pin`).
  - Active PM QoS latency budget enforcement (`config/optid/contracts.toml`) writing dynamic CPU DMA latency floors (`/dev/cpu_dma_latency`) and per-device PCI resume latency floors.
  - **Single-instance exclusive file locking** (`M4` flock) on `optid.lock` to absolutely prevent multi-daemon race conditions.
  - **Robust Signal Hooks (`H2`):** Actively intercepts `SIGTERM`, `SIGINT`, and `SIGHUP` to break the run loop cleanly, guaranteeing deterministic reversion of sysctls (`revert_sysctls`) and PM QoS floors (`revert_pm_qos`) when the daemon is stopped by systemd.
  - **Path Traversal Security Blocks (`M1`):** Structural rejection of any candidate paths containing directory traversal (`..`) components in `guarded_write`.
  - **TOML Crate Integration:** Standardized, highly maintainable deserialization of `policy.toml` via the `toml` crate.
- **Measurement Harness (`crates/rushbench`):** Pure Rust tool capable of executing real energy probing (BAT / Intel RAPL) and responsiveness verification.
- **100% Test-Verified Core:** All 50 pure Rust workspace tests pass cleanly. `validate-doc-sync.py` completely passes.
- **Staged Base OS Overlay (`mkosi/mkosi.extra/`):** Custom release binaries (`optid`, `optctl`, `optid-boot-assess`) have been completely recompiled and staged into Git under their exact target system paths in `mkosi/mkosi.extra/`.

---

## Next Task

You are taking over an exceptionally clean, highly disciplined repository. Your primary mandate is to complete the **v0.5 Minimal Installable System** milestone by delivering the first mkosi-composed Arch Linux disk image that satisfies all four exit criteria.

### Concrete Steps for You:

1. **Verify Workspace Toolchain:**
   Confirm that the stable Rust toolchain is active and run our workspace verification suite:
   ```bash
   cargo test --workspace
   ```
2. **Implement the mkosi/Arch Image Pipeline:**
   Extend and validate `mkosi/mkosi.conf` and `tools/build-mkosi-image.sh` to produce a
   bootable Arch-based disk image that passes `validate-uefi-boot.sh` and `test-rollback.sh`.
3. **Implement a Fresh-Install Test Flow:**
   Create a test script that installs the image onto a blank VM disk (not just boots a
   pre-built image) and verifies the installed system boots twice cleanly.
4. **Verify Server Edition Has No Desktop Dependency:**
   Ensure the server/minimal mkosi profile builds without any desktop packages.
5. **Advance the Roadmap:**
   Upon successful validation of all four exit criteria, update `VERSION` and `ROADMAP.md`
   to certify the **v0.5 Milestone Completed** and advance the project to **v0.6.0-beta.1**.

Happy hacking! Always execute under the **Evidence Rule**.
