# Strategic Ideas Pool

This document tracks strategic directions, candidate features, and architectural pivots for Rush Linux. It serves as a repository for ideas that are beyond the current MVP but critical for the v0.5–v1.0 trajectory.

## 1. Proactive Intent Detection

Currently, `optid` is reactive (monitoring PSI, thermal, and load). We aim to move toward **proactive user-intent awareness**.

### Signal Sources
- **PipeWire:** Detect active video calls (camera/mic usage) or media playback.
- **logind:** Detect session state (active, idle, locked).
- **eBPF:** Monitor build job launches (e.g., `make`, `ninja`, `cargo`) or large file IO.
- **Wayland Compositor Bridges:** Identify foreground applications and fullscreen states.

### Implementation Tradeoffs
- **Wayland Protocol:** A universal `ext-user-intent-v1` protocol would be ideal but requires cross-compositor consensus.
- **Adapters:** Compositor-specific shims (GNOME shell extensions, KWin scripts) provide immediate data but increase maintenance.

## 2. Single-Cog Risk Mitigation

The project's differentiation depends entirely on `optid`. To de-risk this:
- **Conductor Model:** Treat `optid` not just as a sysfs writer, but as the coordinator of kernel mechanisms, systemd, eBPF, and explainability.
- **Coexistence:** Absorbing/integrating with `sched_ext`, `systemd` native automation, and `bpftune` rather than competing with them.

## 3. Differentiation Beyond Optimization (Candidate USPs)

We evaluate six candidate Unique Selling Points:
1. **Immutable-by-design Source Transparency:** Every binary in the UKI is traceable to a signed source recipe.
2. **AI-Native Governance:** Using tools like `docmap.toml`, `Graphify`, and AI-assisted maintenance to ensure project health.
3. **Latency-First Stack Curation:** A distribution where every default (from kernel to terminal emulator) is chosen for minimum latency.
4. **Public Benchmark Lab:** A user-facing feature that runs local benchmarks and compares results to a global baseline.
5. **Developer-First Defaults:** Pre-configured tools for compilation, debugging, and profiling.
6. **Rollback as a First-Class Feature:** Not just for updates, but for policy "experiments" and driver changes.

## 4. Advanced Policy Engine Patterns

- **Continuous Stress Scoring:** Moving from discrete thresholds (e.g., "CPU > 80%") to a continuous score that drives actuation.
- **Batched Actuation with Adaptive Deadband:** Minimizing "knob jitter" by grouping changes and using hysteresis.
- **Zero-Allocation Hot-Path:** Ensuring the core optimizer loop never triggers the allocator.
- **Policy Integrity Verification:** Ensuring loaded policies are signed and haven't been tampered with.
- **Telemetry Privacy Boundaries:** Ensuring benchmark and status data stays local unless explicitly shared.
- **Protected Service Exclusion Lists:** Ensuring critical system services (e.g., SSH, Display Server) are never throttled.

## 5. Strategic Questions for Decision

- **Q1:** Intent detection architecture (universal Wayland protocol vs. compositor-specific adapters)
- **Q2:** `sched_ext` de-risking spike before v1.0?
- **Q3:** GPU/peripheral power policy scope (v1.0 or v1.1?)
- **Q4:** Market position (end-user desktop vs. developer/builder OS)
- **Q5:** Hardware allowlist vs. crowdsourced profiles
- **Q6:** `vm.swappiness` sysctl actuation gap
- **Q7:** Continuous stress score adoption vs. explainability tradeoff
- **Q8:** Zero-allocation hot-path benchmark gate
- **Q9:** Standardized foreground intent protocol contribution
- **Q10:** Telemetry privacy policy and consent model
- **Q11:** System restoration snapshot timing
