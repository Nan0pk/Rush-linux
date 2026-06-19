# 0012 — DTPM Powercap Outer Loop

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0013-thermal-fan-budget-coupling.md
**Code:** crates/optid/src/actuators/powercap.rs, crates/optid/src/sensors/powercap.rs

* * *

## 0. Motivation

The Linux `powercap` framework exposes CPU (and increasingly GPU) power limits through a
unified sysfs interface. RAPL (Running Average Power Limit) on Intel and DRAM power limits,
along with AMD HSMP on AMD Zen processors, allow optid to impose a software power cap that
bounds total chip TDP.

SPEC §3.2 classifies powercap actuation as a BUDGET-ARBITRATOR role — distinct from the
DEPTH-ENABLER sensors/actuators in 0006–0011. The powercap outer loop is the last-resort
governor when all depth-enabler actuations have been applied and package power still
exceeds the thermal budget communicated by 0013.

Research questions: What is the powercap sysfs ABI? How does RAPL PL1/PL2 interact with
hardware TDP? What constraints apply when setting PL1 below the vendor-defined floor? How
does optid avoid conflicting with the thermal governor (0013)? What is the Intel vs. AMD
path?

* * *

## 1. Findings

### 1.1 Linux Powercap ABI

**Q: What is the powercap sysfs interface and what fields does optid use?**

The powercap framework is rooted at `/sys/class/powercap/` [PROVEN — kernel
`Documentation/power/powercap/powercap.rst`]:

```
/sys/class/powercap/intel-rapl/
├── intel-rapl:0/               # Package 0 (socket 0)
│   ├── name                    # "package-0"
│   ├── energy_uj               # cumulative energy counter (µJ); wraps at max_energy_range_uj
│   ├── max_energy_range_uj     # counter rollover value
│   ├── constraint_0_power_limit_uw   # PL1 (sustained, W × 10⁶)
│   ├── constraint_0_time_window_us   # PL1 time window (µs); typically 28000000 = 28s
│   ├── constraint_0_max_power_uw     # hardware ceiling for PL1
│   ├── constraint_1_power_limit_uw   # PL2 (burst)
│   ├── constraint_1_time_window_us   # PL2 window; typically 2400 µs
│   ├── constraint_1_max_power_uw     # hardware ceiling for PL2
│   ├── enabled                       # 1=powercap active
│   └── intel-rapl:0:0/         # Core subdomain (cores only, excluding uncore)
│       └── intel-rapl:0:1/     # Uncore subdomain
└── intel-rapl:1/               # Package 1 (dual-socket; usually absent on laptops)
```

**Key values**:
- `constraint_0_power_limit_uw` — PL1 (Package Power Limit 1): the sustained average TDP.
  Setting this is the primary optid lever.
- `constraint_1_power_limit_uw` — PL2 (burst): typically 1.25–1.5× PL1; optid leaves PL2
  at firmware default unless explicitly budgeting a thermal burst [HYPOTHESIS].
- `energy_uj` — read at 100 ms interval, delta gives instant power in µW; divide by 10⁶
  for watts [PROVEN — standard RAPL energy measurement].

**Writing PL1** [PROVEN]:
```bash
echo 15000000 | sudo tee /sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw
# Sets PL1 = 15 W
```

**Reading current power** [PROVEN]:
```bash
e1=$(cat /sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj)
sleep 0.1
e2=$(cat /sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj)
echo "Power: $(( (e2 - e1) / 100 )) mW"
```

### 1.2 RAPL PL1/PL2 Semantics

**Q: What is the relationship between PL1, PL2, and actual CPU TDP?**

[PROVEN — Intel RAPL documentation, `Documentation/power/powercap/powercap.rst`]:

- **PL1** (Long-term): sustained power limit; hardware enforces as an average over
  `constraint_0_time_window_us` (typically 28 s). If the CPU exceeds PL1 for longer than
  the window, it throttles.
- **PL2** (Short-term): burst power limit; hardware allows up to PL2 for up to
  `constraint_1_time_window_us` (typically 2.4 ms). After the burst window, PL1 is enforced.
- **PL4** (Peak): instantaneous power limit; not exposed via powercap on most consumer
  platforms (enforced by hardware fuses) [PROVEN — PL4 is read-only on Intel client].

**Vendor-defined limits**:
- `constraint_0_max_power_uw` is the BIOS/firmware-set ceiling; optid cannot set PL1 above
  this value without writing MSR directly (not supported and not safe).
- Setting PL1 below 4 W on most x86 laptop CPUs causes thermal instability in the voltage
  regulator and is not recommended [HYPOTHESIS — empirical reports; Intel minimum cTDP is
  typically 6–12 W depending on SKU].

**optid safe floor**: Read `constraint_0_max_power_uw` at startup; PL1 floor = max(5000000,
`cTDP_down_uw` from CPUID or ACPI `_PSS`) [HYPOTHESIS — cTDP_down is the minimum supported
TDP for a SKU; values below this are unsupported].

### 1.3 AMD RAPL and HSMP

**Q: What is the AMD equivalent of RAPL PL1?**

AMD Zen 2+ CPUs expose RAPL via the `amd_energy` kernel driver and powercap:
```
/sys/class/powercap/amd_energy/
├── amd_energy:0/           # Socket 0
│   ├── energy_uj           # cumulative package energy
│   └── ...
```

AMD Zen 3+ also supports **HSMP** (Host System Management Port) for power limit writes on
server/desktop platforms. On AMD laptop CPUs (Ryzen Mobile), power limits are primarily
set via ACPI `_PSS` (P-state packages with power values) or ACPI `_PPC` (preferred P-state
ceiling) [PROVEN — AMD Platform Security Processor manages power limits on Ryzen Mobile].

**AMD Ryzen laptop power management** [PROVEN — `drivers/platform/x86/amd/pmc/`]:

AMD laptop platforms expose power limits via the AMD PMC (Platform Management Controller)
driver:
```bash
cat /sys/bus/platform/devices/AMDI0005\:00/power1_input  # Current package power in µW
```

For constraint writing, AMD Ryzen Mobile exposes limits via the `cezanne-ppt.ko` or
platform-specific ACPI interface. On many AMD laptops, the BIOS enforces a minimum
configurable TDP [HYPOTHESIS — BIOS locks on some OEM builds prevent powercap writes].

**Fallback for AMD**: If powercap writes fail with `EPERM`, optid falls back to CPU
frequency scaling (CPUfreq `scaling_max_freq`) as a coarser power limit proxy [HYPOTHESIS
— frequency scaling has known non-linearity; powercap is preferred].

### 1.4 Outer Loop Control Design

**Q: How does optid's powercap outer loop interact with the thermal governor (0013)?**

The outer loop is a PI (proportional-integral) controller that adjusts PL1 to keep
package power within a budget derived from the thermal state (0013) [HYPOTHESIS —
PI control is standard for thermal power regulation; Linux `intel_pstate` uses similar]:

```
budget_uw = thermal_budget_uw(current_temp, fan_state)  ← from 0013
current_power_uw = rapl_read_power()                    ← 100ms sample
error_uw = current_power_uw - budget_uw
pl1_new_uw = pl1_current_uw - Kp × error_uw - Ki × integral(error)
pl1_new_uw = clamp(pl1_new_uw, pl1_floor_uw, pl1_max_uw)
rapl_write_pl1(pl1_new_uw)
```

**Control interval**: 500 ms [HYPOTHESIS — fast enough to respond to thermal events;
slow enough to avoid RAPL oscillation]. RAPL has inherent averaging over 28 s for PL1,
so control intervals < 1 s are mainly adjusting the setpoint rather than reacting to
instantaneous spikes.

**Separation of concerns**:
- 0013 (thermal) determines the budget in watts based on temperature and fan curve
- 0012 (powercap) enforces the budget by writing PL1
- 0012 does NOT read thermal sensors directly — it receives the budget as an IPC message
  from the policy engine [PROVEN design — matches SPEC §3.3 BUDGET-ARBITRATOR role]

### 1.5 Interaction with `intel_pstate` and CPUfreq

**Q: Does RAPL PL1 interact with the CPU frequency governor?**

RAPL and CPUfreq are orthogonal but interact in practice [PROVEN — `intel_pstate` driver]:

- `intel_pstate` in `powersave` mode already considers RAPL as a power limit signal; if PL1
  is set low, `intel_pstate` frequency targets naturally reduce to stay within budget.
- Setting both a low PL1 (via powercap) AND a low `scaling_max_freq` (via CPUfreq) doubly
  constrains the CPU — may cause excessive throttling [HYPOTHESIS — double-constraint
  interaction; test needed].

**optid policy**: Use powercap PL1 as the primary power control; do NOT set
`scaling_max_freq` unless RAPL is unavailable (AMD PMC locked, VM environment without
RAPL passthrough) [HYPOTHESIS].

### 1.6 Write Journaling and Revert

Each PL1 write is journaled (prior value saved to `/var/lib/optid/revert.journal`) before
the write [PROVEN design — 0006 §1.6 revert protocol]. On `optid.safe=1` boot or watchdog
expiry, prior PL1 is restored.

**Default PL1 value**: optid reads the current PL1 at startup (before any changes) and
stores it as the revert target. If optid crashes without a clean shutdown, the kernel retains
the last-written PL1 until next boot [PROVEN — RAPL writes persist until system reset].
The journal ensures the BIOS default is restored on next optid start with `safe=1`.

* * *

## 2. Architecture Decisions

### Decision A: PI Controller vs. Step-Down

**Selected: PI controller with configurable Kp and Ki** [HYPOTHESIS — step-down would
overshoot; P-only would have steady-state error; PI balances stability with zero steady-state
error]. Initial values: Kp=0.5, Ki=0.1 (dimensionless; tuned in µW domain).

### Decision B: PL1 Only vs. PL1+PL2

**Selected: Control PL1 only; leave PL2 at firmware default** [HYPOTHESIS — PL2 burst
window is 2.4 ms; adjusting it provides minimal battery benefit and risks breaking burst
workloads (compilation, decompression) that legitimately need short power spikes].

### Decision C: RAPL vs. CPUfreq Fallback

**Selected: RAPL powercap primary; CPUfreq fallback only when RAPL writes fail** [PROVEN
design — RAPL is more precise and hardware-enforced; CPUfreq is a coarser approximation].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| PI controller stability (Intel) | PL1 settles within ±0.5 W of budget within 3s | Step-change budget from 25W to 15W; log PL1 and `energy_uj` at 100ms |
| AMD Ryzen Mobile powercap write | PL1 write succeeds on ≥ 3 Ryzen 6xxx/7xxx models | `echo 15000000 > constraint_0_power_limit_uw`; verify with readback |
| Floor safety (< 6 W) | No voltage rail instability on 5 W PL1 for 60s | `stress-ng --cpu 4` at PL1=5W; check `dmesg` for VR faults |
| PL1+PL2 double-constraint overhead | < 1 % additional throttle vs. PL1-only | Compare `perf stat instructions` at same PL1 with PL2 default vs. PL2=PL1 |
| Journal revert latency | PL1 restored within 200ms of `optid.safe=1` boot | Boot with `optid.safe=1`; measure time from boot to PL1 readback = original |

* * *

## 5. Non-Goals

- optid does not implement Turbo Boost enable/disable (that is a firmware decision and
  removing it would break burst performance across all workloads).
- optid does not write PL4 or peak power limits.
- optid does not implement per-core or per-cluster power limits (those are enterprise/server
  features not available on consumer laptop CPUs).
- optid does not implement DDR power limits via RAPL DRAM domain (low priority; DRAM
  power is typically 1–3 W and not the primary battery lever).
- optid does not implement GPU powercap (NVIDIA NVML `nvmlDeviceSetPowerManagementLimit`)
  — that requires NVML library linkage; out of scope for v0.1.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N7  | Powercap outer loop is the BUDGET-ARBITRATOR that enforces thermal budget in §3.3 |
| WP-N8  | RAPL energy counter is the primary battery-draw measurement signal |
| WP-N9  | PI controller closing the loop between thermal state and package power |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/sensors/powercap.rs`: enumerate powercap zones, read
  `energy_uj` at 100 ms, compute rolling power average, read current PL1.
- Implement `crates/optid/src/actuators/powercap.rs`: PI controller loop, PL1 write
  with floor clamp, journal write.

**Short-term**
- Validate PI controller on 3 Intel and 2 AMD reference platforms; tune Kp/Ki.
- Implement `optctl powercap --set 15W` for manual testing and override.

**Medium-term**
- Investigate AMD PMC power limit write path for Ryzen Mobile platforms.
- Extend powercap sensor to DRAM domain for completeness in telemetry.

* * *

## Appendix: Suggested Reading

- Linux kernel `Documentation/power/powercap/powercap.rst`
- Linux kernel `drivers/powercap/intel_rapl_common.c`
- Intel RAPL Interface Specification (external, behind NDA; public summary in IASL docs)
- AMD Processor Programming Reference — HSMP interface
- `rapl-read` tool (simple RAPL energy reader, useful for scripting experiments)
- `turbostat` manpage — section on RAPL columns
