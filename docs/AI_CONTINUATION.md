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

**First, run Dragnet.** Before any feature work, run `python3 tools/dragnet.py
--observe` and read the newest report in `release/evidence/dragnet/`. As of
2026-06-28 the report is GREEN (DRAGNET-008); the v0.3/v0.4/v0.5 evidence
debt closed by PR #174 (2026-06-23) is settled.

**Current milestone: v0.6.0-beta.1 — Hardware-Aware optid.** The
implementation plan lives at
`docs/plans/v0.6-hardware-aware-optid-proposal.md` (5-phase: A through E).

**Phase A is complete** (this branch, 2026-06-28):

- A1 — WP-N4 allowlist foundation verified (12 optid + 4 optctl tests pass).
- A2 — Criterion 4 enumeration harness added
  (`crates/optid/tests/write_site_gating.rs`); 29 write sites inventoried,
  every site classified as `allowlist` / `adr0009-baseline` / `state-file` /
  `revert-path` / `non-sysfs`; drift-detection assertions catch new sites
  added without classification.
- A3 — `--allowlist` default flipped from `disabled` to `enabled`;
  `--no-allowlist` is the new emergency escape hatch. Five new
  `args::tests` pin the new default.
- A5 — research-0006 §7 medium-term plan updated; docmap.toml
  `covers_code` extended to include `crates/optid/src/args.rs`;
  `last_verified` bumped to 2026-06-28.

**Phase B (PPD + GameMode shims) is the next in-container Work Package.**
Per the proposal §3 Phase B, the work is:

1. `crates/optid/src/shim/ppd.rs` — implement `net.hadess.PowerProfiles`
   D-Bus interface on the `io.rushlinux.Optid1` name. GNOME Settings →
   Power slider and KDE `powerdevil` speak this interface.
2. `crates/optid/src/shim/gamemode.rs` — implement
   `com.feralinteractive.GameMode.RegisterGame` / `UnregisterGame` /
   `QueryStatus`. Steam, Lutris, Heroic speak this interface; no client
   changes needed.
3. `crates/optid/src/shim/conflict.rs` — detect `tlp.service`,
   `tuned.service`, `power-profiles-daemon.service` at startup; refuse
   `--apply` if any is running.
4. Tests: `crates/optid/tests/shim_ppd.rs`, `shim_gamemode.rs` —
   exercise the D-Bus interfaces against a mock session bus.

**Phase C (foreground detection + vm.guest class)** can proceed in parallel
with Phase B. Phase C1 (foreground) is feature-flagged off by default
(`--foreground=off`); Phase C2 (vm.guest class) is fully unit-testable
in-container.

**Phase D (reference machines + baselines) is the human/hardware gate.**
Two physical machines need to be nominated by the project owner (one
desktop, one battery-equipped laptop), baselines collected under a
mainstream distro (suggested: Ubuntu 24.04 LTS with PPD `balanced`), then
`optid --apply` runs on the same hardware, and `rushbench` transcripts
committed. The Phase A PR description must surface D1 (machine
nomination) so the project owner has lead time.

### Concrete Steps for You (Phase B start):

1. **Verify Workspace Toolchain:**
   Confirm stable Rust + `PKG_CONFIG_PATH` for libdbus-1-dev:
   ```bash
   cargo test --workspace
   cargo test --test write_site_gating   # Phase A2 harness, must stay green
   cargo test args::                      # Phase A3 default-on regression
   ```
2. **Implement the PPD shim first** (B1) — it's the larger surface but
   has the most compatibility value (every GNOME/KDE user benefits).
3. **Then GameMode** (B2) — smaller surface, TTL-based pin.
4. **Then conflict detection** (B3) — wire `competing_policy_daemons`
   from `config/optid/policy.toml` to an actual startup check.
5. **Run Dragnet after each Phase B sub-PR** to keep evidence green.

Happy hacking! Always execute under the **Evidence Rule**.
