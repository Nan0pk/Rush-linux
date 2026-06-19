# 0013 — Thermal and Fan Budget Coupling

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0012-dtpm-powercap-outer-loop.md
**Code:** crates/optid/src/sensors/thermal.rs, crates/optid/src/policy.rs

* * *

## 0. Motivation

Thermal management couples tightly to power budgeting: when the system is thermally
constrained, the maximum sustainable power (and hence performance) must be reduced. Conversely,
when the system is cool, headroom exists to allow higher burst power without violating junction
temperature limits.

optid needs to read thermal state (CPU/GPU die temperature, skin temperature, fan speed) and
compute a **thermal budget** in watts that is passed to the powercap outer loop (0012). This
budget prevents optid from over-riding hardware thermal throttling signals with power-limit
writes that the hardware will immediately undo.

SPEC §3.3 defines the BUDGET-ARBITRATOR role: optid must not increase power budget when
thermal state says "constrained". The thermal sensor is a CONTRACT-SETTER for the
powercap domain.

Research questions: Which thermal sysfs paths are reliable across Intel and AMD laptop platforms?
What is the difference between `hwmon` temperature sensors and ACPI thermal zones? How does
optid read fan speed? What thermal budget formula produces stable behaviour?

* * *

## 1. Findings

### 1.1 Thermal Sensor Enumeration

**Q: How does optid enumerate and select reliable temperature sensors?**

Linux exposes thermal sensors via two parallel interfaces:

**`hwmon` sysfs** [PROVEN — `Documentation/hwmon/sysfs-interface.rst`]:
```
/sys/class/hwmon/hwmonN/
├── name            # "coretemp", "amdgpu", "nvme", "nct6775", etc.
├── tempM_input     # current temperature in millidegrees Celsius (mdegC)
├── tempM_max       # thermal trip point (high temp warning)
├── tempM_crit      # critical trip (hardware shutdown imminent)
├── tempM_label     # "Core 0", "Package id 0", "CPU die", etc.
└── fan1_input      # fan speed in RPM (if hwmon also exposes fans)
```

**`/sys/class/thermal/` (ACPI thermal zones)** [PROVEN]:
```
/sys/class/thermal/thermal_zoneN/
├── type            # "INT3400 Thermal", "x86_pkg_temp", "acpitz"
├── temp            # current temperature in mdegC
├── mode            # "enabled" | "disabled"
└── trip_point_N_temp / trip_point_N_type
```

**Selection preference for optid**:
1. CPU die/package temperature: prefer `coretemp` hwmon (`name=="coretemp"`) `Package id 0`
   label [PROVEN — coretemp gives per-package and per-core temps with high accuracy]
2. AMD CPU: `k10temp` hwmon (`name=="k10temp"`) `Tctl` or `Tdie` label [PROVEN —
   `Tdie` is the actual junction temperature; `Tctl` includes a platform offset on
   some Ryzen Mobile platforms]
3. ACPI thermal zone `x86_pkg_temp` as fallback if hwmon is unavailable [PROVEN]
4. Board/EC temperatures: `nct6775`, `it87`, `asus_ec` hwmon drivers expose skin/VRM
   sensors; useful for chassis temperature budgeting [PROVEN — driver-dependent]

**CPU temperature label disambiguation** [PROVEN]:
- Intel: `Package id 0` is the package thermal sensor; `Core N` is per-core
- AMD: `Tdie` on Ryzen Mobile is the SoC junction; `Tccd1`/`Tccd2` for chiplet temperature
  (not applicable to single-chiplet mobile SKUs)

### 1.2 Fan Speed Reading

**Q: How does optid read fan speed?**

**hwmon fans** [PROVEN]:
```bash
cat /sys/class/hwmon/hwmon*/fan1_input   # RPM; 0 = fan stopped or not spinning
```

Fan hwmon is exposed by EC (Embedded Controller) drivers: `nct6775`, `asus_ec_sensors`,
`thinkpad_acpi`, `dell-smm-hwmon`, `hp-wmi`, etc. Not all laptops expose fan speed via
hwmon — some keep it proprietary in EC firmware.

**ThinkPad-specific** [PROVEN — `thinkpad_acpi` driver]:
```bash
cat /proc/acpi/ibm/fan   # "speed: 3200" line; more reliable on ThinkPads
```

**ASUS-specific** [PROVEN — `asus_ec_sensors` driver since kernel 5.17]:
```bash
cat /sys/devices/platform/asus_ec_sensors/hwmon/hwmon*/fan1_input
```

**Fallback**: If no hwmon fan sensor is found, optid falls back to reading ACPI thermal
zone trip points as a proxy for fan demand — if `trip_point_N_type == "active"` is
currently triggered, the fan is spinning at ≥ N-th speed level [HYPOTHESIS — coarse proxy;
actual RPM unavailable].

### 1.3 Thermal Budget Formula

**Q: What formula does optid use to compute the power budget from thermal state?**

optid implements a linear thermal derating budget function [HYPOTHESIS — linear is simple
and explainable; non-linear variants can be added later]:

```
T_die = read_cpu_temperature()     # mdegC → °C
T_hi  = config.thermal_hi_c       # default: Tjunction - 10°C (e.g., 95°C for a 105°C part)
T_lo  = config.thermal_lo_c       # default: 60°C (cool; no derating needed)
T_max = config.tjunction_c        # CPU maximum junction temp; read from tempM_crit

if T_die ≤ T_lo:
    budget_uw = pl1_max_uw         # no derating; full budget
elif T_die ≥ T_hi:
    budget_uw = pl1_floor_uw       # maximum derating; minimum sustainable power
else:
    # Linear interpolation
    ratio = (T_die - T_lo) / (T_hi - T_lo)
    budget_uw = pl1_max_uw - ratio × (pl1_max_uw - pl1_floor_uw)
```

**Reading `tempM_crit`** [PROVEN — coretemp exposes hardware Tjunction as `temp1_crit`]:
```bash
cat /sys/class/hwmon/hwmon2/temp1_crit   # e.g., 105000 = 105°C
```

optid sets `T_hi = T_crit - 10°C` by default; this gives a 10°C headroom before the
hardware's own thermal throttle kicks in, allowing optid's powercap to take effect first
and avoid abrupt hardware throttling.

**Hysteresis**: `T_lo` has a 2°C hysteresis band (T_lo_up=60°C, T_lo_dn=58°C) to prevent
budget oscillation when temperature is near the lower threshold [HYPOTHESIS — standard
control hysteresis technique].

### 1.4 Skin Temperature vs. Junction Temperature

**Q: Should optid use skin temperature or junction temperature for the budget?**

**Junction temperature** (CPU `Tdie`/`Package id 0`) is the primary input for optid's
thermal budget [PROVEN — junction temp directly determines thermal throttle; it's fast-
responding to workload changes and accurate].

**Skin temperature** (chassis surface, measured by `nct6775` NTC thermistors or EC sensors)
is a SECONDARY constraint for user comfort [HYPOTHESIS — skin temp > 45°C on palm rest or
keyboard causes user discomfort; value from IEC 62368-1 human touch temperature limits]:

```
if skin_temp_c > config.skin_temp_limit_c:   # default 43°C
    budget_uw = min(budget_uw, skin_budget_uw)  # override with skin-based limit
```

Skin temperature sensor names vary widely by OEM; optid configuration allows specifying
the hwmon sensor path via `thermal.skin_temp_hwmon_path` in `optid.toml` [HYPOTHESIS —
no universal hwmon label for skin temperature].

### 1.5 ACPI Platform Thermal Interface

**Q: Does optid need to interact with the ACPI thermal framework?**

ACPI thermal zones define trip points that trigger cooling actions (fan level changes,
P-state lowering). The kernel ACPI thermal driver handles these automatically [PROVEN —
`drivers/acpi/thermal.c`].

optid does NOT override ACPI thermal actions — doing so could disable the kernel's own
safety throttling [PROVEN design — conservative; ACPI thermal is a safety system].

What optid DOES: reads ACPI `trip_point_N_temp` values at startup to understand the
platform's thermal profile and set appropriate `T_hi` defaults without manual configuration
[HYPOTHESIS — autodetect from ACPI trip points; avoids config burden on per-model basis].

### 1.6 Fan Speed as a Thermal State Proxy

When the CPU temperature is unavailable but fan speed is readable, optid can estimate
thermal state from fan RPM [HYPOTHESIS — empirical curve; platform-specific]:

```
fan_pct = fan_rpm / fan_max_rpm     # 0.0–1.0
# Estimate effective thermal derating from fan load
# (correlation with temperature varies by platform; treat as rough proxy)
budget_factor = 1.0 - 0.5 × max(0, fan_pct - 0.7)
```

This approximation is ONLY used when CPU temperature sensors are unavailable (e.g., running
as a non-root user without access to `coretemp` hwmon) [HYPOTHESIS].

* * *

## 2. Architecture Decisions

### Decision A: Single Temperature Signal vs. Multi-Signal Fusion

**Selected: Primary on CPU die temperature; secondary skin temperature as override**
[PROVEN design — die temperature is the most reliable and universally available signal;
skin is optional enhancement for user comfort budgeting].

### Decision B: Linear Derating vs. PID Temperature Controller

**Selected: Linear derating** for the thermal budget function [HYPOTHESIS — linear is
simpler, more predictable, and easier to debug than a PID thermal controller; the
powercap outer loop in 0012 already has PI control, so double-PID would be difficult to
tune without interaction].

### Decision C: ACPI Thermal Override

**Selected: Do not override ACPI thermal actions** [PROVEN — ACPI thermal is a safety
system; optid derates proactively to avoid reaching ACPI trip points, but if the kernel's
thermal driver fires, optid defers to it].

### Decision D: Fan Control

**Selected: No fan control from optid** [PROVEN — fan control requires EC firmware
cooperation on most laptops; available only on a few OEM platforms (ThinkPad ACPI, Asus
WMI); benefits are small (fan noise) relative to risk (thermal runaway); out of scope].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| Linear budget formula stability | CPU temp stays < T_crit - 5°C under continuous load | `stress-ng --cpu 4` for 10min; log T_die and PL1 at 100ms; confirm no throttle events |
| Skin temperature sensor availability | ≥ 3 of 5 target laptops have readable skin temp | Enumerate hwmon sensors on ThinkPad X1C, Dell XPS, Lenovo IdeaPad, ASUS Zephyrus |
| AMD Tdie vs. Tctl difference | Verify Tdie = Tctl - 10°C on Ryzen 7 6xxx | `sensors` output vs. `k10temp` sysfs; cross-check with `ryzenadj --info` |
| Thermal budget propagation latency | Budget update visible to powercap in ≤ 100ms | Log budget_uw and pl1_uw with timestamps; verify propagation lag |
| Fan RPM proxy accuracy | Fan proxy within ±5°C of die temperature estimate | Compare fan_pct-derived budget with actual T_die reading on same platform |

* * *

## 5. Non-Goals

- optid does not control fan speed directly (see Decision D above).
- optid does not implement ACPI thermal zone mode override (`mode=disabled`).
- optid does not implement liquid cooling control or external thermal modules.
- optid does not profile thermal paste degradation or heatsink contact (hardware condition).
- optid does not implement GPU die temperature gating for the dGPU budget — GPU thermal
  throttling is handled by the NVIDIA/AMD drivers internally (see 0011).
- optid does not implement platform-mode switching (e.g., `platform_profile` sysfs for
  "quiet"/"balanced"/"performance") as a thermal strategy — `platform_profile` is a separate
  mechanism outside the powercap loop.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N7  | Thermal budget is the CONTRACT-SETTER input to the powercap BUDGET-ARBITRATOR (0012) |
| WP-N8  | Die temperature is the primary telemetry signal feeding the thermal model |
| WP-N9  | Skin temperature constraint protects user comfort independent of performance budget |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/sensors/thermal.rs`: enumerate hwmon devices, select die temp
  sensor by label matching, read fan RPM, read `temp_crit` for auto T_hi.
- Implement thermal budget function in `crates/optid/src/policy.rs` with hysteresis.

**Short-term**
- Test thermal budget formula on 3 Intel + 2 AMD reference laptops under sustained `stress-ng`.
- Document skin temperature hwmon paths for ThinkPad, Dell XPS, and ASUS Zephyrus target models.

**Medium-term**
- Evaluate whether `platform_profile` sysfs (kernel 5.13+) should complement or replace
  the thermal budget approach for platforms where EC firmware manages fan/power together.
- Implement `optctl thermal` command for live budget monitoring.

* * *

## Appendix: Suggested Reading

- Linux kernel `Documentation/hwmon/sysfs-interface.rst`
- Linux kernel `drivers/hwmon/coretemp.c` — Intel CPU temperature
- Linux kernel `drivers/hwmon/k10temp.c` — AMD CPU temperature (Tdie vs. Tctl)
- `lm-sensors` / `sensors` tool — hwmon enumeration
- IEC 62368-1 §9.2 — touch temperature limits for user-accessible surfaces
- AMD PPR (Processor Programming Reference) — Tdie offset for Ryzen Mobile
- `acpi_thermal_zone` documentation in `Documentation/ABI/testing/sysfs-class-thermal`
