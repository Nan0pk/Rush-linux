# Apple Power Stack Analysis & Linux Power Management Blind Spots

Status: Superseded as implementation guidance; retained as the historical idea
inventory. Its exact Apple-controller claims, platform-interface assumptions and
power-share numbers are not established measurements. Read the
[source-backed reassessment](0025-os-goals-and-source-build-reassessment.md) and
[platform dispositions](0022-platform-primitives-disposition.md) before acting
on any recommendation below. This preserves the original ideas without treating
them as verified interfaces or authorized controls.

## 1. Apple CLPC is a PID controller, not a simple data flow

Apple's CLPC samples per-thread-group execution metrics, computes a control effort (a scalar representing needed performance), and maps that effort to a recommended core type and DVFS state. It is a closed-loop controller per thread group. The control effort is then clamped by a separate thermal/power loop before reaching hardware.

**Actionable for optid:** Model the PSI→EPP path as a dual-loop controller. Inner loop: per-cgroup performance target (PSI as error signal, EPP as actuator). Outer loop: thermal/power budget (RAPL energy_uj as feedback, package power limit as actuator). These two loops must be separate — merging them causes oscillation.

## 2. XNU Clutch/Edge scheduler

Apple's production scheduler is Clutch (hierarchical thread group bucketing with per-QoS timeshare) with the Edge extension, which adds cross-cluster migration based on weighted edge costs between clusters. It migrates only when scheduling latency delta exceeds edge weight.

**Actionable for optid:** `sched_ext` can replicate this. Edge weights between clusters should be asymmetric and tunable. A hardware probe should measure actual migration cost per platform and write those weights.

## 3. AMD HFI and Dynamic Preferred Core

AMD HFI provides runtime thread classification by hardware. Preferred core rankings dynamically change based on workload, platform conditions, thermals, and aging.

**Actionable for optid:** Subscribe to CPPC `highest_perf` change notifications and update the core preference map at runtime. Use `/sys/devices/system/cpu/cpu*/acpi_cppc/` thread classes to inform EPP writes.

## 4. ARM MPMM (invisible hardware throttling)

Microarchitectural mechanism in Armv9-A cores detects and limits high-activity events to prevent overcurrent.

**Actionable for optid:** Read AMU auxiliary counters to detect MPMM gear-downs. When gear reduction happens without thermal pressure, spread work across cores instead of reducing frequency.

## 5. Idle injection as a first-class actuator

Intel PowerClamp synchronizes forced C-state entry across all online CPU threads, achieving controllable C-state residency.

**Actionable for optid:** Use PowerClamp as the primary thermal response on Intel, and generic `idle_inject` on ARM, rather than strict RAPL hard caps which cause unpredictable frequency dips.

## 6. Peripheral Power (PCIe/NVMe and Display)

PCIe ASPM and NVMe APST fight each other if uncoordinated. Furthermore, display backlight and states (PSR/PSR2) consume 30-50% of system power at idle.

**Actionable for optid:** Coordinate ASPM policy and NVMe APST latency limits based on PSI. Integrate with the display stack (`i915.enable_psr`, RC6 residency) to manage the largest power consumer.

## 7. Event-driven PSI triggers

Polling `/proc/pressure/cpu` wastes CPU.

**Actionable for optid:** Replace the poll loop with `epoll`-able PSI triggers at multiple thresholds (e.g., 10ms/1s for light pressure, 100ms/1s for heavy pressure) for tiered responses.

## 8. Open-Source Analogues (IBM OCC & Qualcomm AOSS)

IBM OCC and Qualcomm AOSS are fully unified firmware power controllers.

**Actionable for optid:** Adopt OCC's architecture: a state machine cycling through Nominal → Turbo → Boost with per-sensor droop detection and an "Active" mode where OS provides workload hints. Use memory bandwidth throttling as a power lever.

## 9. Frequency invariance errors

If the invariance scale factor is wrong, the scheduler picks the wrong core.

**Actionable for optid:** Verify invariance accuracy before trusting `schedutil`. Compare `scaling_cur_freq` to APERF/MPERF-derived actual frequency.

## 10. Memory controller P-states

DRAM bandwidth is often wasted power under memory-idle workloads.

**Actionable for optid:** Write lower memory frequency via vendor-specific interfaces when the workload is compute-bound, and restore it when memory pressure rises.

## 11. Intel LPMD and Workload Type Hints

Intel Low Power Mode Daemon (LPMD) is the closest existing Linux analogue to Apple's CLPC. Workload Type Hints (Meteor Lake) optimize internal V/F curves based on workload type (compute, graphics, media).

**Actionable for optid:** Extend or explicitly replace LPMD functionality. Enable workload hints via sysfs to let hardware classify workloads.

## 12. S0ix / Modern Standby

The largest battery drain gap is sleep. A single device holding a non-zero LTR blocks the entire SoC from entering S0ix.

**Actionable for optid:** Monitor `slp_s0_residency_usec` and `ltr_show`, flag devices blocking S0ix entry, and force `ltr_ignore` on known-safe offenders.

## 13. IRQ affinity as a power lever

Spreading interrupts across all cores prevents deep C-states.

**Actionable for optid:** Auto-detect LP E-cores and migrate non-latency-critical IRQs (USB, audio, Wi-Fi, NVMe completion) to them.

## Summary of the Power Stack

The true scope of an Apple-equivalent system requires orchestrating ~15 actuators:
1. CPU freq (EPP, Boost disable)
2. CPU power (RAPL)
3. CPU idle (Idle injection, Core parking)
4. CPU sched (IRQ affinity, sched_ext)
5. PCIe (ASPM policy)
6. NVMe (APST latency)
7. Display (PSR, DC states)
8. Memory (DRAM freq)
9. Platform (S0ix, Workload hints)
10. Radios (Wi-Fi/Audio power save)

## Power Consumption Breakdown by Workload

- **Idle (5-10W):** Display (30-50%), SoC uncore (10-25%), DRAM (5-10%). CPU cores are <15%.
- **Light Browsing (8-15W):** Display (20-35%), CPU (15-40%), WiFi (5-12%).
- **Video Playback:** Hardware decode shifts power from CPU to GPU media engine, yielding a 2-3x power reduction.
- **Compilation (25-65W):** CPU cores (55-75%). This is the only workload where cpufreq optimization is the dominant lever.
- **Gaming (35-100W+):** dGPU (50-75%).
- **Sleep:** Working S0ix (0.5-3W) vs Broken S0ix (3-8W).

**Conclusion:** The correct mental model is not "optimize CPU frequency for power" but rather "minimize system-wide wakeups and maximize time in the deepest sleep state across all power domains simultaneously."
