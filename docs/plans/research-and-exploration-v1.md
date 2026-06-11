# Research and Exploration Plan v1

This document outlines the exhaustive research phase required to inform the strategic decisions for Rush Linux v0.5–v1.0. The goal is to move from "strategic questions" to "evidence-based decision candidate" by conducting technical spikes, competitive analysis, and upstream alignment reviews.

## 1. Governance & Core Pillars

- **Core Pillar:** Adaptive optimization (`optid`) as the conductor.
- **Goal:** Every research item must answer: "How does this strengthen the core optimization mission?"

## 2. Decision-Making Framework (Policy)

To ensure decisions are robust, Rush Linux adopts a **data-driven, multi-criteria decision analysis (MCDA)** approach.

### 2.1 Roles (DACI)
- **Driver:** The AI Agent or Contributor conducting the research.
- **Approver:** Nan0pk (The project owner).
- **Contributors:** Community members, upstream maintainers (e.g., PipeWire/systemd developers).
- **Informed:** Users and future maintainers.

### 2.2 Decision Criteria & Weighting
Every proposal will be evaluated against these five criteria:
1. **Responsiveness (Weight: 35%):** Measured reduction in p99 latency or felt jitter.
2. **Power Efficiency (Weight: 25%):** Measured reduction in package power (Watts) for equivalent work.
3. **Upstream Alignment (Weight: 20%):** Adherence to modern Linux standards (e.g., UKI, cgroup v2, Wayland).
4. **Maintenance Cost (Weight: 15%):** Long-term burden of code, fragmentation, or vendor-specific shims.
5. **Safety & Reversibility (Weight: 5%):** Risk of hardware damage or inability to roll back.

### 2.3 Complexity Categorization (Cynefin)
Before research begins, categorize the question:
- **Simple:** Best practice exists. (Research = Audit).
- **Complicated:** Expert analysis needed. (Research = Expert consult/Review).
- **Complex:** Emergent behavior, no clear answer. (Research = Technical spike/Benchmarking).
- **Chaotic:** Immediate action needed to stabilize. (N/A for strategic review).

## 3. Research Tracks (Q1–Q11 & Beyond)

*Execution for these tracks is tracked in the **[v0.4.5: Strategic Research Milestone](https://github.com/Nan0pk/Rush-linux/milestone/1)**.*

### [Track A: Intent & Interaction](https://github.com/Nan0pk/Rush-linux/issues/37) (Q1, Q9, Q4)
- **Objective:** Move from reactive to proactive optimization.
- **A1 (Architecture):** Audit existing Wayland protocols (`idle-inhibit`, `xdg-activation`, `tablet-v2`). Investigate `portal` APIs for application metadata.
- **A2 (Foreground):** Prototyping `logind` session tracking vs. a lightweight Wayland compositor shim.
- **A3 (Personas):** Competitive analysis of "Performance OS" market (NixOS, CachyOS, Clear Linux). Define the "Developer/Builder" persona requirements.

### [Track B: Kernel & Scheduler](https://github.com/Nan0pk/Rush-linux/issues/38) (Q2, Q6, Q8)
- **Objective:** Prove the felt-responsiveness thesis.
- **B1 (sched_ext):** Benchmark `scx_lavd` and `scx_bpfland` against standard EEVDF under "Rush-like" mixed loads (e.g., 4K video + background compile).
- **B2 (Memory):** Analyze MGLRU performance with varying `vm.swappiness` levels specifically in ZRAM-backed scenarios.
- **B3 (Hot-Path):** Profiling `optid` core loop for allocations and jitter. Explore `no_std` for critical paths.

### [Track C: Hardware & Power](https://github.com/Nan0pk/Rush-linux/issues/39) (Q3, Q5)
- **Objective:** Safe, cross-vendor power management.
- **C1 (GPU/Peripherals):** Catalog sysfs knobs for AMD/Intel/NVIDIA GPU power levels. Audit `nvme` APST and USB autosuspend impact on latency.
- **C2 (Allowlist):** Review TLP and `power-profiles-daemon` hardware databases. Research "Security of Tunables" (which knobs can brick hardware?).

### [Track D: Reliability & Data](https://github.com/Nan0pk/Rush-linux/issues/40) (Q7, Q10, Q11)
- **Objective:** Trustworthy, explainable, and private optimization.
- **D1 (Control Theory):** Compare discrete state-machine transitions vs. PID/Continuous-Stress-Score transitions for responsiveness.
- **D2 (Telemetry):** Research Prio/Differential Privacy for local-first telemetry.
- **D3 (Rollback):** Benchmarking snapshot overhead on `ext4` (via `reflink` or `dm-thinp`) vs. `btrfs`.

## 4. Methodology: The Research Memo

For each research point, the output must be a **Research Memo** (recorded in `docs/agent-decisions/`) containing:

1.  **Context:** The strategic question and its complexity class.
2.  **Hypothesis:** What we expect to find.
3.  **Methodology:** Steps taken (e.g., "Ran `bench-optid-matrix.sh` on HP Victus").
4.  **Evidence & Data:** Graphs, CSVs, logs, or upstream documentation links.
5.  **Option Comparison:**
    - Option A: [Pros/Cons/Score]
    - Option B: [Pros/Cons/Score]
    - Option "Do Nothing": [Pros/Cons/Score]
6.  **Pre-Mortem Analysis:** "If we pick Option A and it fails in 6 months, why did it fail?"
7.  **Decision Hint:** Recommended path for Nan0pk.
8.  **Reversal Plan:** How do we undo this if it fails?

## 5. Execution Schedule (Phased)

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

## 6. Success Criteria

Research is complete when:
- Each of Q1–Q11 has a corresponding Research Memo.
- A "Decision Matrix" is produced, mapping research findings to ADR candidates.
- No "Core Pillar" is compromised by a proposed decision.
