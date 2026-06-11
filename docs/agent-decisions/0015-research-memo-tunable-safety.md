# Research Memo 002: Safety of Tunables

| Field | Value |
|---|---|
| ID | RM-002 |
| Strategic Questions | Q5, C2 |
| Track | Track C: Hardware & Power |
| Complexity Class | Simple |
| Date | 2026-06-12 |
| Driver | Arena Agent |

## 1. Context & Hypothesis
Rush Linux must ensure that `optid` never damages hardware while applying optimizations. We hypothesize that standard kernel scaling drivers (EPP, platform_profile) have built-in safety bounds, while "enthusiast" tunables (voltage, clock multipliers) pose a high risk of "bricking" or degradation.

## 2. Methodology
- **Audit:** Searched for known "dangerous" sysfs paths in mainstream drivers.
- **Review:** Checked TLP and `power-profiles-daemon` exclusion lists.

## 3. Evidence & Data
- **Safe Paths:**
    - `/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference` (Architectural bounds).
    - `/sys/firmware/acpi/platform_profile` (Vendor-defined safe modes).
    - `/sys/class/drm/card*/device/power_dpm_force_performance_level` (Safe profile switching).
- **Dangerous Paths:**
    - **Undervolting:** `/sys/class/hwmon/hwmon*/device/voltage_offset` or similar (Can cause immediate system hang or long-term instability).
    - **GPU Overclocking:** `/sys/class/drm/card*/device/pp_od_clk_voltage` (Requires `amdgpu.ppfeaturemask` usually, but extremely dangerous if automated).
    - **Thermal Trip Points:** Manually overriding `/sys/class/thermal/thermal_zone*/trip_point_*_temp` (Risk of fire or hardware melting if safety shutoffs are delayed).

## 4. Option Comparison

| Option | Pros | Cons | MCDA Score (H/M/L) |
|---|---|---|---|
| **A: Open Policy** | Maximum performance/efficiency gains. | High liability, support nightmare. | **L** (Too risky) |
| **B: Strict Allowlist** | Near-zero risk of damage. | Misses some hardware-specific wins. | **H** (Recommended) |
| **C: Signed Profiles** | Safe but flexible. | High infrastructure cost. | **M** (Future goal) |

## 5. Pre-Mortem Analysis
**Failure Scenario:** If a user uses `optid` to set a "Performance" mode and the hardware overheats because a fan driver was blocked or failed, the user blames Rush Linux.
**Mitigation:** `optid` must always respect `thermal_zone` signals and *never* write to fan-speed or thermal-trip knobs. It should only request performance levels; the hardware/firmware must handle the cooling.

## 6. Decision Hint
- **Q5 (Allowlist):** Adopt a **Hard Allowlist**. `optid` should only be capable of writing to a pre-defined set of architectural knobs.
- **Hardware Profile Safety:** Any knob that can bypass hardware safety (thermal/voltage) is banned from the default service.

## 7. Reversal Plan
If we find we are too restrictive, we can move a knob from the "Banned" to "Allowlist" only after three independent verifications on that specific hardware model.
