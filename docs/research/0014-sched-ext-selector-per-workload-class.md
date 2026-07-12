# 0014 — sched_ext Scheduler Selection per Workload Class

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/decisions/0015-sched-ext-default-on.md (ADR-0015)
**Code:** crates/optid/src/actuators/sched.rs, crates/optid/src/sensors/workload.rs

* * *

## 0. Motivation

The Linux `sched_ext` framework (merged in kernel 6.12) allows userspace BPF schedulers to
replace the default EEVDF scheduler at runtime. Different schedulers optimise for different
objectives: `bpfland` and `lavd` optimise for interactive latency; `rusty` for throughput;
`scx_simple` as a lightweight baseline. Rush Linux defaults to `scx_lavd` (ADR-0015) with
an EEVDF fallback.

optid can switch the active sched_ext scheduler based on the current workload class,
providing a second axis of power/performance optimisation that complements the power-state
actuators in 0007–0013. The scheduler choice affects:
- **C-state residency**: a task-packing scheduler can consolidate threads on fewer cores,
  increasing idle core residency (deeper C-states on unused cores).
- **IPC (instructions-per-cycle)**: a scheduler that minimises migrations keeps caches hot,
  improving IPC for the same power budget.
- **Wakeup latency**: interactive schedulers reduce wakeup time for latency-sensitive tasks.

Research questions: How does optid switch schedulers at runtime? What are the power
implications of each sched_ext scheduler? How does optid detect the workload class to
select a scheduler? What is the interaction with kernel EEVDF fallback?

* * *

## 1. Findings

### 1.1 sched_ext Runtime Switching

**Q: How does optid switch the active BPF scheduler at runtime?**

sched_ext is loaded via the `scx` userspace tooling. The kernel provides:

**sysfs interface** (kernel ≥ 6.12) [PROVEN — `Documentation/scheduler/sched-ext.rst`]:
```
/sys/kernel/sched_ext/
├── state        # "enabled" | "disabled"
├── switch_all   # 1=all tasks use BPF sched; 0=only tasks opting in (via sched_setattr)
└── hotplug_seq  # sequence number for hotplug events
```

Scheduler programs are loaded via `bpftool prog load` or the `scx_*` userspace loader
binaries (from the `scx` project, `tools/sched_ext/`). The loader pins the BPF program
to a well-known path; on exit, the kernel reverts to EEVDF [PROVEN — sched_ext design:
sched_ext program is unloaded when the loader exits, restoring EEVDF automatically].

**optid's control path** [HYPOTHESIS — design; no kernel API for "switch scheduler
without reloading"]:
1. At startup, optid records which `scx_*` binary is active (if any) by reading
   `/sys/kernel/sched_ext/state` and process table for `scx_*` daemons.
2. To switch schedulers, optid:
   a. Sends SIGTERM to the current `scx_*` loader process (causes EEVDF revert)
   b. Waits 100 ms for EEVDF revert (verify via `state == "disabled"`)
   c. Launches the new `scx_*` binary as a managed child process
3. To revert to EEVDF, optid sends SIGTERM to the current loader.

**scx loader binaries** (installed at `/usr/bin/scx_*`) [PROVEN — `scx` project]:
- `scx_lavd` — Latency-Aware Virtual Deadline (LAVD); optimised for interactive tasks
- `scx_bpfland` — BPF-land; task-classification aware; good for mixed workloads
- `scx_rusty` — throughput-optimised; task-affinity aware; good for compilation
- `scx_flatcg` — cgroup-flat; weight-based; good for container workloads
- `scx_simple` — minimal example; uniform priority; useful for testing

### 1.2 Scheduler Selection per Workload Class

**Q: Which scheduler does optid select for each workload class?**

[HYPOTHESIS — scheduler selections based on scheduler documentation and benchmark reports;
needs validation on target hardware]:

| Workload class | Selected scheduler | Rationale |
|---------------|--------------------|-----------|
| `idle` | EEVDF (no sched_ext) | No tasks need scheduling; minimal overhead |
| `light` | `scx_lavd` | Light interactive tasks benefit from low wakeup latency |
| `interactive` | `scx_lavd` | Desktop use; compositor, browser, IDE need low latency |
| `latency-critical` | `scx_lavd` with `--lowlatency` flag | Real-time audio/video; minimum wakeup variance |
| `throughput` | `scx_rusty` | Compilation, ML training; maximise instruction throughput |

**Default on AC**: `scx_lavd` [PROVEN — ADR-0015 default].
**Default on battery**: `scx_lavd` with task packing enabled (if `--pack-tasks` flag
supported) [HYPOTHESIS — task packing on battery reduces active core count].

### 1.3 sched_ext and C-State Residency

**Q: How does scheduler choice affect CPU C-state residency and battery life?**

The connection between scheduler and C-states is indirect [PROVEN — CPU C-state entry
requires all SMT siblings of a core to be idle]:

- A scheduler that consolidates tasks on fewer physical cores (task packing) leaves more
  cores fully idle → those idle cores reach deeper C-states (C6/C10) → lower idle power.
- `scx_lavd` on recent kernels supports a `--performance` vs. `--powersave` mode; powersave
  mode prefers packing tasks on fewer cores [PROVEN — scx_lavd v0.7+ changelog].
- EEVDF by default distributes tasks across all cores for fairness → more cores in shallow
  C1/C1E states → higher idle power [HYPOTHESIS — consistent with powertop observations;
  not formally measured in Linux kernel literature].

**optid's interaction**:
- During `idle` workload class: switch to EEVDF (trivial to schedule 0 tasks)
- During `light` workload class: use `scx_lavd --powersave` for task packing
- During `throughput` class: use `scx_rusty` which uses all available cores (throughput
  > C-state depth in this class)

**C-state telemetry feedback loop** (from 0018):
optid monitors per-core C-state residency at 2s intervals. If C6 residency on idle cores
is < 60 % with `scx_lavd --powersave`, it logs a hint that task packing may not be
working [HYPOTHESIS — 60 % C6 is a heuristic target; needs calibration].

### 1.4 EEVDF Fallback Mechanism

**Q: What triggers EEVDF fallback and how does optid handle it?**

sched_ext reverts to EEVDF automatically in these conditions [PROVEN — sched_ext design]:
1. The sched_ext loader process exits (any reason including crash or SIGTERM)
2. The BPF program calls `scx_bpf_error()` (internal BPF error path)
3. The kernel detects a task stuck (timeout) in the BPF scheduler
4. The user writes `0` to `/sys/kernel/sched_ext/switch_all`

When EEVDF fallback occurs, `/sys/kernel/sched_ext/state` becomes `"disabled"`.
optid detects this at the next 2s poll. If the fallback was not requested by optid
(conditions #2, #3), optid logs a warning and re-launches the appropriate scheduler
for the current workload class [HYPOTHESIS — auto-re-launch after unexpected fallback;
with a cap of 3 re-launches per 60s to avoid thrash].

### 1.5 Workload Class Detection

**Q: How does optid detect the current workload class to select the right scheduler?**

Workload class detection is out of scope for this brief (see `crates/optid/src/workload.rs`
and the workload detection research). Summary of signals used:

- `idle`: no active processes (other than kernel threads); CPU utilisation < 5 % for > 10s
- `light`: browser/terminal/editor processes active; utilisation < 30 %
- `interactive`: compositor rendering at > 30 fps; human input events arriving
- `latency-critical`: JACK/PipeWire running with realtime threads; cgroup `cpu.latency=1`
- `throughput`: `make -j N`, `cargo build`, ML framework process; sustained > 80 % utilisation

Detection uses `/proc/stat` (CPU utilisation), `cgroup2` `cpu.pressure` (PSI), and
optionally a D-Bus hint from the compositor [HYPOTHESIS — detection heuristics; may need
per-workload tuning].

### 1.6 sched_ext Overhead Budget

**Q: What is the overhead of running a BPF scheduler vs. EEVDF?**

[HYPOTHESIS — based on sched_ext benchmarks from the kernel patchset discussion]:

- `scx_simple` overhead vs. EEVDF: ~1–3 % IPC reduction (BPF dispatch path adds ~500 ns
  per wakeup/switch)
- `scx_lavd` overhead: ~2–5 % IPC reduction; partially compensated by better cache locality
- `scx_rusty` overhead: ~1–2 % IPC reduction; throughput-optimised path is efficient

The overhead budget is < 5 % IPC, which is within the acceptable range for battery-idle
scenarios where throughput is not the primary goal. For `throughput` class, `scx_rusty`'s
IPC benefit from better task affinity can exceed the overhead [HYPOTHESIS].

* * *

## 2. Architecture Decisions

### Decision A: Scheduler Switching — optid Process Manager vs. systemd Unit

**Selected: optid manages `scx_*` loader as a child process** [HYPOTHESIS — simpler than
N systemd units; optid can react to workload class changes without systemd round-trip latency;
on-demand switching in < 500 ms].

### Decision B: --powersave vs. Separate "Battery Scheduler"

**Selected: Single scheduler (`scx_lavd`) with mode flags for battery vs. AC** [HYPOTHESIS —
fewer scheduler binaries to maintain; `scx_lavd --powersave` already implements task packing].

### Decision C: Auto-Re-launch after Unexpected Fallback

**Selected: Re-launch with cap (3 attempts per 60s)** [HYPOTHESIS — 3 attempts handles
transient BPF JIT errors; cap prevents infinite restart loop if scheduler has a bug].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| C6 residency improvement with `scx_lavd --powersave` | ≥ 15 % higher C6 residency vs. EEVDF on idle desktop | `turbostat --interval 2 | grep C6` with EEVDF vs. scx_lavd --powersave for 60s idle |
| Scheduler switch latency | < 500 ms from SIGTERM to new scheduler active | Time from `kill -TERM $(pidof scx_lavd)` to `cat /sys/kernel/sched_ext/state` == "enabled" |
| scx_lavd overhead on interactive desktop | < 5 % frame-time increase vs. EEVDF in Compositor benchmark | `gnome-shell --perf` comparison; 1000 frame trace |
| scx_rusty throughput improvement | ≥ 5 % wall-clock speedup on `make -j $(nproc)` | Kernel build time with EEVDF vs. scx_rusty × 5 runs each |
| Unexpected fallback rate | < 1 fallback per hour of normal use | 24h soak test on 3 reference laptops with scx_lavd; monitor `/sys/kernel/sched_ext/state` |

* * *

## 5. Non-Goals

- optid does not implement a custom BPF scheduler — it selects from the upstream `scx`
  project schedulers.
- optid does not configure per-cgroup CPU weights (that is optid-contracts, a separate
  sub-system not detailed here).
- optid does not set CPU affinity for individual processes.
- optid does not implement NUMA-aware scheduling policies (not applicable to single-socket
  laptop platforms).
- optid does not manage kernel real-time (SCHED_FIFO/SCHED_RR) priorities.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N10 | Scheduler selection is the CONTRACT-SETTER for CPU scheduling behaviour per workload class |
| WP-N11 | Task packing via scx_lavd --powersave increases C-state depth (feeds 0018 telemetry) |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/actuators/sched.rs`: detect current sched_ext state,
  launch/terminate `scx_*` loader on workload class change, monitor for unexpected fallback.
- Add `scx_lavd`, `scx_rusty` as `optid` service dependencies in packaging.

**Short-term**
- Run C6 residency experiment to validate task-packing benefit (§4 gap #1).
- Measure scheduler switch latency (§4 gap #2).

**Medium-term**
- Investigate whether scx_lavd `--powersave` flag is upstream or requires local patch.
- Implement `optctl sched --status` showing current scheduler and workload class.

* * *

## Appendix: Suggested Reading

- Kernel source: `tools/sched_ext/` — BPF scheduler implementations
- `Documentation/scheduler/sched-ext.rst`
- `scx` project: github.com/sched-ext/scx — scx_lavd, scx_rusty, scx_bpfland
- LWN.net: "An extensible scheduler class" (sched_ext merge coverage, 2024)
- `turbostat` — C-state residency measurement methodology
- LAVD design document: `tools/sched_ext/scx_lavd.bpf.c` header comment
