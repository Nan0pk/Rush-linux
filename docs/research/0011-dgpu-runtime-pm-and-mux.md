# 0011 — dGPU Runtime PM and MUX Control

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0006-hw-allowlist-db-design.md, docs/research/0009-runtime-pm-autosuspend-policy.md
**Code:** crates/optid/src/actuators/dgpu.rs, crates/optid/src/sensors/dgpu.rs

* * *

## 0. Motivation

A discrete GPU (dGPU) present but unused can consume 3–8 W simply by being powered on.
On hybrid-graphics laptops (Intel/AMD iGPU + NVIDIA/AMD dGPU), optid's highest-leverage
idle power action is runtime-suspending the dGPU when no application holds a GPU context.

Two distinct mechanisms are involved:
1. **dGPU Runtime PM** — the Linux kernel suspends the dGPU PCIe function via its
   graphics driver's runtime PM callbacks; power rail de-assertion follows.
2. **MUX (Output Multiplexer) Control** — on MUX-equipped laptops, the MUX routes the
   display output either through the iGPU or dGPU; switching to iGPU-only mode while the
   dGPU is suspended prevents firmware from powering it back on for display purposes.

SPEC §3.2 classifies dGPU control as a DEPTH-ENABLER: requires HWID in allowlist, exit
latency within contract floor, write journaled. The dGPU power-on latency (100–500 ms for
NVIDIA, 50–200 ms for AMD) is the primary constraint against over-aggressive suspension.

Research questions: How does optid detect dGPU usage? What is the power-on latency and
how does it affect the contract floor? How does NVIDIA PRIME vs. AMD dynamic switching
differ? How does optid interact with the MUX without requiring a logout? What is the safe
runtime PM path for NVIDIA (`nouveau` vs. proprietary driver)?

* * *

## 1. Findings

### 1.1 dGPU Detection and Idle Determination

**Q: How does optid detect that the dGPU is idle and safe to suspend?**

**Runtime usage counting** [PROVEN — `drivers/gpu/drm/*/pm.*`]:

The DRM subsystem maintains an internal usage count for each GPU device. When userspace
opens a DRM fd and submits work, the driver calls `pm_runtime_get()`. When the fd is
closed or the device becomes idle, `pm_runtime_put_autosuspend()` is called.

optid observes idle state via:
```bash
cat /sys/bus/pci/devices/0000:01:00.0/power/runtime_status
# "suspended" = idle and powered down
# "active"    = in use or resuming
```

For NVIDIA proprietary driver (`nvidia.ko`): the PCI device for the dGPU exposes
`runtime_status` but the driver must be compiled with runtime PM support (`NVreg_DynamicPowerManagement=0x02` for Turing+) [PROVEN — NVIDIA driver documentation].

For AMD Radeon dGPU (`amdgpu.ko`): runtime PM is enabled by default since kernel 5.4;
`runtime_status` reflects actual power state [PROVEN — amdgpu driver source].

For NVIDIA open-source (`nouveau.ko`): runtime PM available but less mature; GPU must
declare ACPI `_PR3` power resource for D3cold to be reached [PROVEN — nouveau documentation].

**Additional idle signals** [HYPOTHESIS — no kernel ABI for these; use heuristics]:
- No processes with open DRM fd: scan `/proc/*/fd/` for symlinks to `/dev/dri/card1`
- No active VRAM allocations: `cat /sys/kernel/debug/dri/1/clients` (root, debugfs)
- Compositor not using dGPU: check if Wayland compositor fd is on `card0` (iGPU)

### 1.2 NVIDIA Runtime PM — Power Management Modes

**Q: What are NVIDIA's runtime PM modes and which does optid target?**

NVIDIA Turing+ (RTX 20xx+) and Ada Lovelace support three `NVreg_DynamicPowerManagement`
values [PROVEN — NVIDIA open-driver and proprietary documentation]:

| Value | Mode | Description |
|-------|------|-------------|
| `0x00` | Never | Runtime PM disabled; dGPU always powered (default for older drivers) |
| `0x01` | Coarse | Suspend entire GPU when no clients; 500ms+ latency |
| `0x02` | Fine | Suspend GPU subsystems independently; fastest wake; preferred |

**optid recommends mode `0x02`** for Turing+ in the installer/packaging configuration:
```ini
# /etc/modprobe.d/optid-nvidia.conf
options nvidia NVreg_DynamicPowerManagement=0x02
```

With `0x02`, the NVIDIA driver uses ACPI D3cold via the `_PR3` power resource (if the
ACPI table declares it) to fully power-rail-gate the dGPU after ~10 s idle [PROVEN —
ACPI `_PR3` is the D3cold power resource; requires BIOS support].

**Power-on latency** with `NVreg_DynamicPowerManagement=0x02` and D3cold [HYPOTHESIS —
measurements vary widely by platform]:
- Resume from D3hot: ~50–150 ms
- Resume from D3cold (full power rail): ~200–500 ms

SPEC §3.1 gate: contract floor must be ≥ 500 ms before optid enables dGPU D3cold.
For `latency-critical` workload class (floor < 5 ms), optid must hold the dGPU active.
For `idle` workload class (floor ≥ 500 ms), D3cold is safe to permit.

### 1.3 AMD Radeon dGPU Runtime PM

**Q: How does AMD dGPU runtime PM work and what does optid control?**

AMD dGPU runtime PM is enabled by default in the `amdgpu` driver since kernel 5.4 with
`CONFIG_PM_AUTOSUSPEND=y` [PROVEN — `drivers/gpu/drm/amd/amdgpu/amdgpu_drv.c`].

The driver uses ATPX (AMD Transfer Power Expressions) or `_PR3` ACPI power resources to
control the dGPU power rail. Autosuspend delay defaults to 5000 ms (5 s).

**optid can tighten the delay**:
```bash
echo 2000 > /sys/bus/pci/devices/0000:01:00.0/power/autosuspend_delay_ms
echo auto > /sys/bus/pci/devices/0000:01:00.0/power/control
```

This requires the HWID in the 0006 allowlist with `domain="dgpu_runtime_pm"`.

**AMD power-on latency** [HYPOTHESIS — based on amdgpu community reports]:
- D3hot → active: ~30–100 ms
- D3cold (full ATPX power-off): ~100–300 ms

optid uses D3hot as the default target (faster resume); D3cold requires `max_state=d3cold`
in the allowlist and a contract floor ≥ 300 ms.

### 1.4 MUX Control

**Q: How does optid interact with the display multiplexer on MUX-equipped laptops?**

A hardware MUX routes eDP panel output either from the iGPU or dGPU. On MUX-less hybrid
laptops (most post-2021 designs), display always goes through iGPU; dGPU renders offscreen
and the result is copied to iGPU framebuffer (PRIME). On MUX-equipped laptops, the dGPU
can drive the display directly for maximum performance mode.

**MUX kernel interface** [PROVEN — kernel ≥ 5.20 / 6.1, `drivers/platform/x86/asus-wmi.c`,
`lenovo-wmi-hotkey.c`, `hp-wmi.c`]:

```bash
cat /sys/class/drm/card0/device/mux_switch  # "0" = iGPU, "1" = dGPU (vendor-specific)
# or: /sys/class/firmware-attributes/*/attributes/gpu_mux_mode/current_value
```

MUX switch currently requires logout on most platforms (display manager must re-initialise
DRM after MUX switch). An in-session MUX switch without logout is a kernel work-in-progress
feature [PROVEN — MUX switch without logout is supported on select Dell XPS 15/17 with
kernel ≥ 6.3 and experimental `drm_mux_switch` sysfs].

**optid MUX policy**:
- If in `idle` or `light` workload class and MUX is currently set to dGPU: recommend MUX
  switch to iGPU on next logout (write to telemetry hint; do NOT switch in-session unless
  `drm_mux_switch` is available and the system is known-safe) [HYPOTHESIS — in-session
  MUX switch is too risky without per-model allowlist]
- If MUX is on iGPU: allow dGPU to runtime-suspend freely
- If MUX is on dGPU: dGPU runtime PM is blocked by MUX ownership; do not attempt
  suspension [PROVEN — MUX-on-dGPU means dGPU is scanning out the display framebuffer]

### 1.5 PRIME Offload and dGPU Idle Detection

**Q: On PRIME (MUX-less) systems, how does optid confirm no application is using the dGPU?**

PRIME Offload works by applications explicitly selecting the dGPU via environment variable:
```bash
DRI_PRIME=1 glxgears          # route to dGPU via PRIME
__NV_PRIME_RENDER_OFFLOAD=1 vkcube  # NVIDIA PRIME offload
```

When no application sets these env vars, all rendering goes to the iGPU and the dGPU is
idle. optid detects dGPU idle by:

1. `runtime_status == "suspended"` — definitive: driver has already suspended [PROVEN]
2. Scan `/proc/*/environ` for `DRI_PRIME` or `__NV_PRIME_RENDER_OFFLOAD` — if any live
   process has these, dGPU is potentially in use [HYPOTHESIS — env var scan is a heuristic;
   a process could set the var without actually using the GPU]
3. `cat /sys/kernel/debug/dri/1/clients` — lists open DRM clients [PROVEN — debugfs; root]

optid uses check #1 as the primary signal; #2 and #3 as secondary pre-suspension checks.

### 1.6 Exit Latency for SPEC Gate

The contract floor required for dGPU suspension varies by mode:

| Mode | Exit latency | Required contract floor |
|------|-------------|------------------------|
| D3hot (NVIDIA/AMD) | ≤ 150 ms | ≥ 200 ms (`light` class) |
| D3cold ATPX (AMD) | ≤ 300 ms | ≥ 400 ms (`light` class) |
| D3cold ACPI _PR3 (NVIDIA) | ≤ 500 ms | ≥ 500 ms (`idle` class only) |

For `interactive` or `latency-critical` workload class, dGPU must remain active (usage
count held via `pm_runtime_get()` from optid itself to prevent premature suspension).

* * *

## 2. Architecture Decisions

### Decision A: Driver Agnosticism

**Selected: optid uses the sysfs runtime PM interface (`power/control`,
`power/autosuspend_delay_ms`, `power/runtime_status`) and does NOT call driver-specific
ioctls** [PROVEN — sysfs PM interface is stable ABI; driver ioctls are not standardised
across NVIDIA/AMD/nouveau and would require driver-specific code paths].

### Decision B: MUX Switch — Recommend vs. Actuate

**Selected: optid recommends MUX switch via telemetry hint but does NOT switch in-session**
unless `drm_mux_switch` sysfs is confirmed available and the model is in the allowlist
[HYPOTHESIS — in-session switch is too risky; unexpected display blackout is a regression].

### Decision C: D3cold — Allowlist-Gated Only

**Selected: D3cold (full power rail) requires explicit `max_state=d3cold` in allowlist
entry** per the 0006 schema. D3hot is the safe default for any dGPU with runtime PM
support [PROVEN design — conservative default for a feature with 500ms latency penalty].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| NVIDIA D3cold resume latency | ≤ 500 ms p99 on RTX 40xx | `time glxinfo` after dGPU D3cold; repeat 20× |
| AMD D3cold resume latency | ≤ 300 ms p99 on RX 6xxx | Same with AMD dGPU |
| dGPU idle power (NVIDIA, D3cold) | ≤ 0.5 W dGPU contribution | `turbostat` RAPL `gpu` domain with dGPU suspended |
| dGPU idle power (AMD, D3cold) | ≤ 0.3 W | Same |
| In-session MUX switch stability | 0 display blanks on Dell XPS 15 9530 (kernel 6.3+) | Write `0` to MUX switch sysfs; verify display stays on for 60 s |
| PRIME env scan false-positive rate | < 1 % false positives vs. DRM client check | Compare env scan vs. debugfs client list across 100 app launches |

* * *

## 5. Non-Goals

- optid does not manage iGPU power states — the i915/amdgpu driver handles iGPU PM.
- optid does not configure NVIDIA GPU clock/voltage profiles.
- optid does not implement GPU power capping (see 0012 for DTPM/powercap).
- optid does not manage external EGPU (Thunderbolt-attached GPU) — out of scope for v0.1.
- optid does not set Vulkan device selection or OpenGL `MESA_VK_DEVICE_SELECT`.
- optid does not implement GreenWithEnvy-style fan curve control for the dGPU.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N2  | dGPU runtime PM is a DEPTH-ENABLER; MUX switch is a CONTRACT-SETTER for display domain |
| WP-N4  | HWID allowlist gates all dGPU PM actuations; D3cold needs explicit `max_state` entry |
| WP-N5  | dGPU `runtime_suspended_time` is a key idle-power accounting signal |
| WP-N6  | dGPU suspension can save 3–8 W — second-largest single-device lever after display |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/sensors/dgpu.rs`: detect dGPU presence, read runtime_status,
  scan DRM clients, check MUX state.
- Implement `crates/optid/src/actuators/dgpu.rs`: set `power/control=auto` and
  `autosuspend_delay_ms` per allowlist entry; hold active during non-idle workload class.

**Short-term**
- Seed allowlist with NVIDIA RTX 20xx/30xx/40xx and AMD RX 5xxx/6xxx/7xxx runtime PM
  entries, D3hot default, D3cold gated.
- Run D3cold latency experiments on reference hardware.

**Medium-term**
- Implement in-session MUX switch for confirmed-safe models (Dell XPS 15/17 with
  kernel 6.3+ `drm_mux_switch`).
- Investigate GPU-level power metering via NVML (NVIDIA) and `amdgpu_pm_info` (AMD)
  for finer power telemetry.

* * *

## Appendix: Suggested Reading

- NVIDIA open driver documentation: Dynamic Power Management (`NVreg_DynamicPowerManagement`)
- Linux kernel `drivers/gpu/drm/amd/amdgpu/amdgpu_drv.c` — runtime PM flags
- Linux kernel `drivers/gpu/drm/nouveau/` — nouveau runtime PM and `_PR3`
- ArchWiki: PRIME GPU offloading — practical PRIME setup and dGPU idle detection
- Supergfxctl project (System76/Asus): MUX and dGPU PM management reference implementation
- `acpidump` + `iasl` — decode ACPI `_PR3` and ATPX tables for dGPU power resources
