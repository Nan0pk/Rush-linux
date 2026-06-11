# Research and Exploration Plan v1

This document outlines the exhaustive research phase required to inform the strategic decisions for Rush Linux v0.5–v1.0. The goal is to move from "strategic questions" to "evidence-based decision candidate" by conducting technical spikes, competitive analysis, and upstream alignment reviews.

## 1. Governance & Core Pillars

- **Core Pillar:** Adaptive optimization (`optid`) as the conductor.
- **Goal:** Every research item must answer: "How does this strengthen the core optimization mission?"

## 2. Research Tracks (Q1–Q11 & Beyond)

### Track A: Intent & Interaction (Q1, Q9, Q4)
- **Objective:** Move from reactive to proactive optimization.
- **A1 (Architecture):** Audit existing Wayland protocols (`idle-inhibit`, `xdg-activation`, `tablet-v2`). Investigate `portal` APIs for application metadata.
- **A2 (Foreground):** Prototyping `logind` session tracking vs. a lightweight Wayland compositor shim.
- **A3 (Personas):** Competitive analysis of "Performance OS" market (NixOS, CachyOS, Clear Linux). Define the "Developer/Builder" persona requirements.

### Track B: Kernel & Scheduler (Q2, Q6, Q8)
- **Objective:** Prove the felt-responsiveness thesis.
- **B1 (sched_ext):** Benchmark `scx_lavd` and `scx_bpfland` against standard EEVDF under "Rush-like" mixed loads (e.g., 4K video + background compile).
- **B2 (Memory):** Analyze MGLRU performance with varying `vm.swappiness` levels specifically in ZRAM-backed scenarios.
- **B3 (Hot-Path):** Profiling `optid` core loop for allocations and jitter. Explore `no_std` for critical paths.

### Track C: Hardware & Power (Q3, Q5)
- **Objective:** Safe, cross-vendor power management.
- **C1 (GPU/Peripherals):** Catalog sysfs knobs for AMD/Intel/NVIDIA GPU power levels. Audit `nvme` APST and USB autosuspend impact on latency.
- **C2 (Allowlist):** Review TLP and `power-profiles-daemon` hardware databases. Research "Security of Tunables" (which knobs can brick hardware?).

### Track D: Reliability & Data (Q7, Q10, Q11)
- **Objective:** Trustworthy, explainable, and private optimization.
- **D1 (Control Theory):** Compare discrete state-machine transitions vs. PID/Continuous-Stress-Score transitions for responsiveness.
- **D2 (Telemetry):** Research Prio/Differential Privacy for local-first telemetry.
- **D3 (Rollback):** Benchmarking snapshot overhead on `ext4` (via `reflink` or `dm-thinp`) vs. `btrfs`.

## 3. Methodology

For each research point, the output must be a **Research Memo** containing:
1. **The Question:** (e.g., Q1).
2. **Methodology:** (What was audited/built/measured).
3. **Findings:** (Raw data, code snippets, upstream feedback).
4. **Decision Hint:** (Recommended path based on data).
5. **Impact on Core Pillar:** (How it helps `optid`).

## 4. Execution Schedule (Phased)

### Phase R1: Foundations (The "Low-Hanging" Spikes)
- B1 (`sched_ext` benchmark).
- B2 (Swappiness/ZRAM audit).
- A1 (Wayland protocol audit).
- C2 (Allowlist/Knob safety audit).

### Phase R2: Prototyping (The "Proof of Value")
- A2 (Logind/Wayland shim prototype).
- B3 (Zero-allocation audit).
- D1 (Continuous score simulator).
- C1 (GPU sysfs mapping).

### Phase R3: Final Alignment
- D2 (Privacy model design).
- D3 (Snapshot timing benchmarks).
- A3 (Market position finalization).

## 5. Success Criteria

Research is complete when:
- Each of Q1–Q11 has a corresponding Research Memo.
- A "Decision Matrix" is produced, mapping research findings to ADR candidates.
- No "Core Pillar" is compromised by a proposed decision.
