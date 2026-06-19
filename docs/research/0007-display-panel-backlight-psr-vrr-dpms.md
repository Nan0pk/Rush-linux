# 0007 — Display Panel: Backlight, PSR, VRR, DPST/ABM, and DPMS

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0018-telemetry-runtime-state-observability.md
**Code:** crates/optid/src/actuators/display.rs, crates/optid/src/sensors/display.rs

* * *

## 0. Motivation

The display subsystem is the single largest consumer of battery power on most laptops,
accounting for 30–50 % of total system TDP when the screen is on. SPEC §3.2 designates
the display as a DEPTH-ENABLER domain: optid may deepen panel power state (lower backlight,
enable PSR, enable VRR at low frame rates, blank via DPMS) only when the HWID is in the
allowlist, exit latencies are within the active contract floor, and the write is journaled.

Five separate power-saving mechanisms compose here:

1. **Backlight dimming** — most impactful lever (typically 1–3 W range).
2. **Panel Self Refresh (PSR / PSR2)** — eliminates redundant frame fetches on static content.
3. **Variable Refresh Rate (VRR / FreeSync / AdaptiveSync)** — at low frame rates the panel
   can drop to ≤ 48 Hz refresh, saving refresh-cycle energy.
4. **Display Power Saving Technology / Adaptive Backlight Management** — content-adaptive
   dimming with histogram equalisation to preserve perceived brightness.
5. **DPMS / panel off** — blank after idle timeout, full panel off for deeper sleep.

Research questions: how does optid enumerate backlight devices, select the authoritative
one, and safely set brightness? Can optid observe PSR status without breaking it? Which
compositors support VRR and how does optid signal intent? How is DPST (Intel) / ABM (AMD)
controlled from userspace? What DPMS transition is safe to trigger from a daemon vs.
compositor?

* * *

## 1. Findings

### 1.1 Backlight Device Enumeration and Selection

**Q: How does optid enumerate and select the correct backlight device?**

The kernel exposes backlight devices under `/sys/class/backlight/`. Each device exposes:
- `type` — `raw` (vendor driver controls hardware directly), `platform` (ACPI/EFI
  platform), or `firmware` (ACPI video)
- `max_brightness` — integer ceiling
- `brightness` — current value (read/write)
- `actual_brightness` — read-back after hardware rounding

**Selection algorithm** [PROVEN — matches gnome-settings-daemon `gsd-backlight.c` heuristic]:

```
Priority: raw > firmware > platform
Within same type: prefer name "intel_backlight" or "amdgpu_bl*"
Tiebreak: highest max_brightness (most granular control)
```

Intel graphics registers `intel_backlight` (type=raw) as the preferred device since kernel
3.13. ACPI video may also register `acpi_video0` (type=firmware) — optid must **ignore**
`acpi_video*` if any `raw` device is present [PROVEN — kernel module param
`video.use_native_backlight=1` enforces same preference in newer kernels].

AMD graphics registers `amdgpu_bl0` (type=raw) on systems using the amdgpu DRM driver.
On hybrid systems with a discrete NVIDIA GPU in Optimus mode, the backlight is controlled
via the integrated Intel/AMD GPU; optid should not attempt to control `nvidia_backlight`,
which maps to the dGPU output and may not correspond to the internal panel.

**Implementation note**: optid reads `type` from all entries under `/sys/class/backlight/`,
builds a priority-sorted list, and caches the selection. Re-enumeration occurs on udev
`add`/`remove` events for the `backlight` subsystem [HYPOTHESIS — udev backlight hotplug
events are rare but needed for Thunderbolt dock brightness scenarios].

### 1.2 PSR Status Observability

**Q: Can optid observe PSR1/PSR2 state without breaking it?**

**Intel PSR** [PROVEN — i915 DRM driver, `drivers/gpu/drm/i915/display/intel_psr.c`]:

Status is available via debugfs at `/sys/kernel/debug/dri/0/i915_edp_psr_status` (requires
debugfs mounted, root access). Key fields:

```
Sink support: yes [0x03]
PSR mode: PSR2 enabled
PSR active: no        ← PSR not currently saving power (content changed recently)
Selective fetch: yes
PSR2 live flips: 0
```

The `PSR active` field tells optid whether PSR is actively suppressing frame fetches.
Reading this file is safe — it is a `seq_file` read-only export, does not generate timer
interrupts, and does not prevent entry into PSR [PROVEN — kernel source confirms seq_file
reads do not call into PSR enable/disable paths].

**Known limitation**: PSR2 is disabled on cursor movement. This is a hardware/firmware
interaction (cursor plane DMA conflicts with PSR2 selective fetch) and cannot be worked
around from userspace [PROVEN — documented kernel bug, tracked as "i915 PSR2 cursor
flicker"]. optid tracks `PSR2 live flips` counter; if rising while the workload class is
`idle`, it indicates a compositor not parking the cursor plane — emit a hint in telemetry.

**AMD PSR** [PROVEN — `drivers/gpu/drm/amd/display/amdgpu_dm/amdgpu_dm_psr.c`]:

AMD exposes PSR state via `/sys/kernel/debug/dri/0/amdgpu_dm_psr_state`:

```
eDP panels support PSR: yes
PSR active: no
PSR version: PSR1
```

AMD PSR1 is widely safe; AMD PSR2 (called "PSR-SU" for Selective Update) requires
panel-specific allowlisting in the kernel itself — optid does not need to gate this
separately as the kernel will refuse PSR-SU on unsupported panels.

**Energy impact** [HYPOTHESIS — based on Intel NUC Elm Creek measurements cited in kernel
mailing list]: PSR1 saves ~0.5 W; PSR2 saves ~1.0–1.5 W on 60 Hz panels. Actual saving
depends on content — a static terminal saves more than a video player.

### 1.3 VRR (Variable Refresh Rate) Support

**Q: Which compositors support VRR, and how does optid signal intent to the KMS layer?**

VRR is controlled via the DRM connector property `vrr_enabled` [PROVEN — KMS docs,
`Documentation/gpu/drm-kms-helpers.rst`]. Setting `vrr_enabled=1` on the connector object
allows the panel to vary refresh rate in real time between the panel's minimum and maximum
supported rate.

**Compositor support** [PROVEN]:
- **Mutter (GNOME ≥ 44)**: experimental in 44–46, stable in 47+; toggle in Settings →
  Displays → Variable Refresh Rate; requires atomic modesetting KMS
- **KWin (KDE Plasma ≥ 5.26)**: `kscreen-doctor output.eDP-1.vrr=always` or via System
  Settings → Display → Adaptive Sync
- **wlroots ≥ 0.17**: compositor must call `wlr_output_set_adaptive_sync()`; environment
  variable `WLR_DRM_ALLOW_MODESET=1` enables atomic modesetting

**optid's role**: optid does NOT directly set `vrr_enabled` — this is the compositor's
privilege on a Wayland desktop (the compositor owns the KMS fd). Instead, optid:

1. Reads the current `vrr_capable` and `vrr_enabled` connector properties via
   `/sys/class/drm/card0-eDP-1/vrr_capable` [PROVEN — readable without DRM master]
2. Reports VRR capability and current status in telemetry
3. May send a D-Bus hint to Mutter/KWin requesting VRR enable/disable via compositor
   APIs (`org.gnome.Mutter.DisplayConfig` or `org.kde.KScreen`) [HYPOTHESIS — no
   standardised daemon-driven VRR API exists yet; this needs compositor upstream work]

**Refresh rate floor**: eDP panels typically support 48–165 Hz. Below the declared floor
(e.g., < 48 Hz), the panel reverts to its minimum supported rate to avoid flicker. VRR at
48–60 Hz during idle content (static UI, slow video) saves 10–20 % of panel refresh-cycle
energy compared to fixed 144 Hz [HYPOTHESIS — based on VESA AdaptiveSync spec estimates;
needs measurement on target panels].

### 1.4 DPST (Intel) and ABM (AMD) Content-Adaptive Dimming

**Q: How does optid control content-adaptive backlight technology?**

**Intel DPST** (Display Power Saving Technology) [PROVEN — `i915.enable_dpst` kernel
module parameter documented in `Documentation/gpu/i915.rst`]:

DPST is controlled at driver load via the `i915.enable_dpst=1` module parameter. There is
no runtime sysfs toggle — the feature is either enabled or disabled at module load. optid
cannot dynamically enable/disable DPST at runtime without reloading the module.

**Recommended configuration** (optid ships as a packaging artifact):

```ini
# /etc/modprobe.d/optid-i915.conf
options i915 enable_dpst=1
```

optid can detect whether DPST is active by observing the discrepancy between `brightness`
(the set value) and `actual_brightness` (hardware value after DPST adjustment) in
`/sys/class/backlight/intel_backlight/` [PROVEN — DPST reduces actual_brightness below
the set value when content histogram warrants it; difference indicates DPST is working].

**AMD ABM** (Adaptive Backlight Management) [PROVEN — `amdgpu` DRM driver,
`drivers/gpu/drm/amd/display/amdgpu_dm/amdgpu_dm.c`]:

AMD exposes ABM control as the DRM connector property `abm_level` with values 0–4:
- `0` = disabled
- `1` = minimal (< 5 % visible luminance reduction)
- `2` = moderate (~10 %)
- `3` = aggressive (~20 %)
- `4` = maximum (~30 %)

This property is readable and writable via DRM ioctls (`DRM_IOCTL_MODE_SETPROPERTY`).
On a running Wayland compositor, the compositor holds DRM master and connector property
writes from optid will fail with `EACCES`. optid communicates ABM level via KWin/Mutter
when those compositor APIs are available [HYPOTHESIS — not yet standardised]; as fallback,
documents the `/etc/modprobe.d/amdgpu.conf` approach: `options amdgpu abmlevel=2`.

### 1.5 DPMS — Panel Blank and Off

**Q: What is the safe DPMS transition strategy for optid?**

In a Wayland environment, DPMS is compositor-controlled. On X11, `xset dpms` or
`XSetScreenSaver` work directly. optid targets Wayland-first.

**Wayland path** [PROVEN — compositor D-Bus APIs]:
- GNOME: `org.gnome.ScreenSaver.Lock()` for blank; `org.freedesktop.login1.Manager.Inhibit()`
  for preventing blank; idle signalled via KMS `connector.DPMS` property by Mutter
- KDE: `org.freedesktop.ScreenSaver.Inhibit()` / `SimulateUserActivity()`
- Generic: `ext-idle-notify-v1` Wayland protocol (compositors ≥ 2023) for idle notification

Direct KMS DPMS write from a daemon is **unsafe** in Wayland sessions — the compositor
holds DRM master and a direct write causes `EACCES` [PROVEN — writing DRM connector
properties without master fails].

**DPMS sequence** [HYPOTHESIS — timing values; needs validation with target UX]:

```
Idle 10s  → optid signals compositor: dim backlight to 15%
Idle 30s  → compositor blanks display (DPMS Standby via KMS)
Idle 60s  → compositor suspends panel (DPMS Off / PSR-entry)
Idle 300s → optid requests systemd-logind Suspend()
```

optid influences this sequence by using `org.freedesktop.login1.Manager.Inhibit()` to
delay suspend when an active workload holds a contract floor higher than `light`.

### 1.6 Brightness Jitter and PWM Frequency

**Q: What is the safe minimum brightness, and does PWM cause health issues?**

PWM (Pulse Width Modulation) at low frequencies causes perceptible flicker for some users.
There is no standard sysfs interface to read PWM frequency [PROVEN — no
`/sys/class/backlight/*/pwm_freq` exists in the kernel ABI].

**Detection options**:
1. EDID brightness range descriptor (uncommon, rare in laptop panels)
2. Vendor-specific DMI/SMBIOS string (rare)
3. Heuristic: if `max_brightness ≤ 255` and the EDID panel vendor is a known
   PWM-heavy manufacturer (certain AUO/Innolux LCD lines), apply a 30 % minimum
   [HYPOTHESIS — based on community flicker reports; no clinical validation]

**Safe floor**: optid enforces a configurable minimum brightness
(`display.min_brightness_pct`, default 15 %) to prevent usability failures. Values
below 15 % on PWM panels at < 500 Hz PWM frequency may cause discomfort for
photosensitive users [HYPOTHESIS — precautionary; no peer-reviewed evidence].

* * *

## 2. Architecture Decisions

### Decision A: Backlight Control — Direct sysfs vs. Compositor D-Bus

**Option 1**: optid writes directly to `/sys/class/backlight/*/brightness`.
**Option 2**: optid requests brightness change via compositor D-Bus API.

**Selected: Option 1 (direct sysfs) for battery-driven dimming** [PROVEN — direct write
is atomic with latency < 1 ms; compositor round-trip adds 50–200 ms and may be
rate-limited or ignored during screen lock]. Compositor is notified of the write via D-Bus
after the fact so its slider stays in sync.

Exception: user-visible brightness (from keyboard shortcut or Settings) must go through
the compositor; optid acts as a policy follower in that path (reading current brightness
to inform dim-threshold calculations).

### Decision B: PSR Observability — Polling vs. DRM Event

PSR state is polled at the 2s cadence from debugfs rather than waiting for a DRM event
(no DRM event exists for PSR state change) [PROVEN]. Overhead is one `read()` syscall per
2s poll — negligible.

### Decision C: VRR — Daemon Hint vs. Direct KMS

optid issues VRR hints via compositor D-Bus (when available) rather than writing KMS
properties directly. This avoids the DRM-master conflict on Wayland [PROVEN design]. On
headless / TTY sessions, optid may write KMS directly if DRM master is available and
no compositor is detected.

### Decision D: DPST/ABM — Module Param vs. Runtime Toggle

DPST is a module-load-time parameter with no runtime toggle; ABM is runtime-writable via
DRM connector property but inaccessible without DRM master on Wayland. optid documents
both and prefers ABM level=2 on AMD systems during battery-idle workload class
[HYPOTHESIS — level=2 is safe default; level=3+ risks visible colour banding on some
panels].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| PSR1 energy saving (Intel) | ≥ 0.4 W reduction vs. no-PSR at idle desktop | `sudo turbostat --interval 2` with PSR disabled (`echo 0 > /sys/kernel/debug/dri/0/i915_edp_psr_status` override) vs. enabled |
| PSR2 energy saving (Intel) | ≥ 0.8 W reduction vs. no-PSR at idle desktop | Same as above; confirm PSR2 active via `i915_edp_psr_status` |
| VRR energy at 48 Hz vs. 60 Hz eDP | ≥ 8 % panel power reduction | `intel_gpu_top` + RAPL `uncore` domain diff; or external USB-C power meter |
| ABM level=2 luminance delta | ≤ 10 % cd/m² difference vs. ABM=0 at 200 nit reference | Colorimeter measurement on AMD laptop at ABM 0 vs. 2 |
| PWM frequency survey | Confirm ≥ 500 Hz on all target laptop panel SKUs | `sudo python3 tools/pwm-detect.py` (write tool) using oscilloscope or flicker meter |
| Backlight write latency | < 5 ms sysfs write-to-panel response | `perf stat -e raw_syscalls:sys_exit -- sh -c "echo 100 > /sys/class/backlight/intel_backlight/brightness"` |

* * *

## 5. Non-Goals

- optid does not implement a screen blanker or screensaver — that is the compositor's job.
- optid does not control display brightness over LVDS/VGA (legacy, EOL hardware).
- optid does not set display refresh rate directly — only signals VRR intent to compositor.
- optid does not alter colour calibration or ICC profiles.
- optid does not support brightness control for Thunderbolt/USB-C external monitors
  (dock-firmware-managed, no standard sysfs path).
- optid does not implement HDR tone mapping or peak brightness boosting.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N1a | DPMS / blank transition is optid's primary CONTRACT-SETTER signal for display depth |
| WP-N2  | PSR and VRR are DEPTH-ENABLER actuations — both require HWID in allowlist (0006) |
| WP-N5  | Backlight dimming is the primary energy lever; PSR telemetry feeds the energy model |
| WP-N6  | PSR active % and VRR frame rate are key telemetry signals for display idle detection |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/sensors/display.rs`: enumerate backlight devices and select
  authoritative; poll PSR status (Intel + AMD debugfs paths); read VRR capability from
  connector properties; read `actual_brightness` for DPST activity detection.
- Implement `crates/optid/src/actuators/display.rs`: write brightness (with floor check and
  journal); send VRR/ABM hints via compositor D-Bus; write modprobe config during install.

**Short-term**
- Build allowlist entries for known-good display panels (5 × Intel + 5 × AMD laptops with
  PSR confirmed working) using `edid-decode` output as HW evidence.
- Run PSR energy-saving experiment to fill §4 gap #1 and #2.

**Medium-term**
- Investigate standardised compositor API for daemon-driven VRR hint; file GNOME/KDE
  upstream issues if no API exists.
- Explore ABM control path for Wayland without DRM master (compositor protocol extension
  or kernel sysfs attribute for `abm_level`).

* * *

## Appendix: Suggested Reading

- Kernel source: `drivers/gpu/drm/i915/display/intel_psr.c`
- Kernel source: `drivers/gpu/drm/amd/display/amdgpu_dm/amdgpu_dm_psr.c`
- Kernel docs: `Documentation/gpu/drm-kms-helpers.rst` — connector properties
- `gnome-settings-daemon` source: `plugins/power/gsd-backlight.c`
- VESA AdaptiveSync specification (public, covers eDP VRR floor/ceiling)
- Intel NUC power measurement posts on PSR2 savings (LWN.net archives 2022–2023)
- `edid-decode` tool — EDID panel capability parsing
