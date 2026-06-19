# 0007 — Display Panel: Backlight, PSR, VRR, DPMS

_This document is a **research WIP** specifying how optid manages the display panel stack
(backlight, PSR, VRR, DPMS) via an `optid-display-bridge` user-session service. This is
the panel side of WP-N7. The dGPU runtime PM and MUX switching are covered by research 0011.
Tagged `[PROVEN]` (kernel source, upstream docs) or `[HYPOTHESIS]` (plausible, unmeasured)._

**Status:** WIP — design complete, energy measurement experiments pending.
**Author:** Nan0pk
**Date:** 2026-06-19
**Depends on:** SPEC-northstar.md, ADR-0013, research 0005 (focus-bridge pattern reuse)
**Enables:** 0011 (dGPU + MUX, reuses bridge pattern)

* * *

## 0. Motivation

On a typical laptop doing light work, the display subsystem (panel + backlight + eDP link +
controller) is **40–60% of the avoidable energy budget**. PSR2 saves 0.5–1.5 W during
static-screen idle. VRR drop to 48 Hz on static content saves 5–10% of panel+link power.
Backlight is 70–90% of LCD panel power; perceptual dimming (Intel DPST, AMD ABM) recovers
another 5–20% transparently. The display is where optid's biggest per-watt wins are.

Yet the compositor owns the display pipeline. optid cannot directly call KMS ioctls. Every
display-side lever requires bridging the privileged/unprivileged boundary. This research
specifies the `optid-display-bridge` pattern — a `systemd --user` service that reads DRM
debugfs for state, writes backlight via sysfs (root), and emits D-Bus hints to the compositor
for VRR/DPMS policy. **optid supplies policy; the compositor owns the modeset.**

This slot covers panel + backlight + PSR + VRR + DPMS only.
dGPU runtime PM and MUX switching are slot 0011.

* * *

## 1. Findings

### 1.1 Backlight Device Selection

**Multiple entries under `/sys/class/backlight/`** **[PROVEN]**

Drivers that can register a backlight device:
- `intel_backlight` — registered by `drivers/gpu/drm/i915/display/intel_backlight.c` on
  Intel platforms
- `amdgpu_bl0`, `amdgpu_bl1` — registered by AMD display driver
- `acpi_video0`, `acpi_video1` — registered by `drivers/acpi/video.c` (ACPI firmware path)
- `nvidia_wmi_ec_backlight` — NVIDIA WMI EC backlight (some ThinkPads)
- `dell_backlight` — Dell-specific EC backlight

**Selection heuristic (confirmed from gnome-settings-daemon power plugin)** **[PROVEN]**

Priority order (highest first):
1. `intel_backlight` or `amdgpu_bl*` — vendor GPU driver backlight (most accurate)
2. `nvidia_wmi_ec_backlight` — WMI EC path (for NV laptops without ACPI video)
3. `acpi_video0` where type != `acpi_video` (some platforms expose the right device here)
4. `acpi_video0` as last resort (often controls nothing on modern laptops)

The kernel's own selection logic is in `drivers/acpi/video_detect.c::acpi_video_get_backlight_type()`.
It returns `acpi_backlight_vendor` if a vendor driver (i915, amdgpu) has registered, else
`acpi_backlight_video`. gnome-settings-daemon follows the same heuristic: prefer the
device with the highest `max_brightness` value that matches the active connector.

**On a MUX laptop (iGPU + dGPU)** **[HYPOTHESIS]**

When the MUX is in iGPU-output mode, `intel_backlight` or `amdgpu_bl0` is active. When
switched to dGPU-direct, the dGPU's backlight device controls the panel. optid must re-select
the active backlight device after a MUX switch. Detection: compare the connector's
`drm_connector.backlight` pointer via sysfs or poll for which device responds to writes.

**Practical approach:** optid writes to the backlight device with highest numeric
`max_brightness` among vendor-specific entries, ignoring `acpi_video*`.

### 1.2 Perceptual Backlight Dimming (DPST / ABM)

**Intel DPST (Display Power Saving Technology)** **[PROVEN]**

Controlled via kernel module parameter `i915.enable_dpst=1`. Not exposed as a runtime KMS
connector property in upstream kernels (as of 6.9). The DPST status is visible in:
`/sys/kernel/debug/dri/0/i915_dpst_disable` (1=disabled, 0=enabled).

DPST works by boosting the histogram equalization in the display controller, allowing the
backlight to be reduced by 5–20% while maintaining perceived brightness. The driver handles
the histogram analysis; the user sees only a slight contrast shift in very dark scenes.

Enabling/disabling DPST at runtime: write to the debugfs file (root-only). This is a
kernel-internal API and may change. For v0.x: document as `--dpst=on|off` flag in optid
with a note that the interface is not stable.

**AMD ABM (Adaptive Backlight Management)** **[PROVEN]**

Exposed as a KMS DRM connector property `abm_level` with range 0–4:
- 0: ABM disabled (full power)
- 1: Minimal reduction (~5%)
- 2: Low reduction (~10%)
- 3: Medium reduction (~15%)
- 4: Maximum reduction (~20%)

The property is written via `DRM_IOCTL_MODE_SETPROPERTY` (KMS ioctl, requires DRM master).
Since optid is not the compositor, it cannot call this ioctl directly. Instead: optid emits
the desired ABM level via the D-Bus display-hint interface; the compositor (or a dedicated
KMS helper running as DRM master) applies it.

Alternative: the `amdgpu_abm` sysfs path (if exposed by the driver). Check:
`/sys/bus/pci/devices/<igpu-bdf>/amdgpu_abm_level` — not guaranteed to exist but present on
some kernels.

**Default policy** **[HYPOTHESIS]**

On by default for workload classes `idle`, `light`, `interactive`. Off for
`latency-critical` (games/video calls — image quality matters) and when fullscreen video is
detected. This matches the intent of these features (transparent savings for typical use).

### 1.3 PWM vs DC Dimming

**PWM backlight flicker risk** **[PROVEN — widely documented]**

Panels using PWM (pulse-width modulation) for low-brightness control exhibit flicker at
frequencies below ~1 kHz that causes eye strain in sensitive users. DC dimming (constant
current, amplitude modulation) does not flicker.

The kernel does not expose PWM frequency via any standard sysfs attribute. Some vendor
drivers expose `pwm_period` or `bl_hw_require_gpio` but there is no standard ABI.
Detection requires measurement with a photodiode or slow-motion camera.

**Floor constraint for optid** **[HYPOTHESIS — health/safety non-goal]**

Per SPEC §5 (avoiding user-visible harm), optid enforces a minimum brightness floor when
the laptop is known to use PWM: **minimum brightness = 30% of max_brightness**. This is
documented per-HWID in a `pwm_floor_pct` allowlist field.

Without hardware measurement, mark all HWIDs as `pwm_floor_pct = 0` (no floor) in the
seeded baseline. Users can add entries after verifying their panel's PWM behavior.

### 1.4 Ambient Light Sensor (ALS)

**Kernel IIO interface** **[PROVEN]**

ALS exposed via `/sys/bus/iio/devices/iio:deviceN/` with attributes:
- `in_illuminance_input` (lux, calibrated — preferred)
- `in_illuminance_raw` (raw ADC value — needs calibration)
- `in_intensity_both_raw` (visible + IR — less accurate)

The attribute name varies by driver (`in_illuminance_input` is the standard, but some drivers
only expose `in_illuminance_raw`).

**Cooperation model with the compositor** **[HYPOTHESIS]**

The compositor (GNOME: `gnome-settings-daemon` power plugin, KDE: PowerDevil) already reads
ALS and adjusts brightness. optid should NOT compete with this. Instead:

- optid provides a *floor* (minimum brightness given battery state): on battery + ambient < 50
  lux → floor = 20% of max_brightness.
- The compositor provides the *preference* (user-adjusted brightness above the floor).
- Effective brightness = max(optid_floor, compositor_preference).

optid communicates the floor via `org.rush.Optid.BrightnessFloor(pct)` D-Bus signal to
the bridge. The bridge hints the compositor; compositor applies max(floor, preference).

### 1.5 PSR1 / PSR2 / Panel Replay

**PSR modes** **[PROVEN — kernel i915 and amdgpu docs]**

- **PSR1** (Panel Self-Refresh, eDP 1.3): panel captures a framebuffer and self-refreshes
  while the display link is in low-power mode. Link power savings only (not panel power).
  Supported by most eDP 1.3+ panels.
- **PSR2** (eDP 1.4): selective framebuffer update — only changed regions are transmitted.
  Saves link + controller power. Requires panel + GPU + driver to support selective updates.
  Broken by cursor movement (GPU must update the cursor overlay region constantly).
- **Panel Replay** (DP 2.0): similar to PSR2 but for DisplayPort, less eDP-specific.

**Intel PSR observability** **[PROVEN]**

`/sys/kernel/debug/dri/0/i915_edp_psr_status` — text file with fields:
```
Sink support: yes [0x03]
Source support: yes
PSR mode: PSR2 enabled - LRR: No
Source PSR ctl: enabled [0x81904009]
Source PSR2 ctl: enabled [0x83080000]
Busy frontbuffer bits: 0x00000000
PSR2 selective fetch: disabled
Main link in standby mode: yes
Hardware Enabled & Active bit: yes
```

The `Main link in standby mode: yes` line confirms PSR2 is active (link powered down).
optid reads this file every 2 s (part of the 0018 telemetry extension for GPU state).

**AMD PSR observability** **[PROVEN]**

`/sys/kernel/debug/dri/0/amdgpu_dm_psr_info` or `amdgpu_dm_psr_state`:
```
eDP0: PSR active
eDP0: PSR2 supported: yes
eDP0: PSR currently enabled: yes
```

**Module parameters:**
- Intel: `i915.enable_psr=0|1|2` (0=off, 1=PSR1, 2=PSR2); default varies by kernel version
- AMD: `amdgpu.dpm=1` (general DPM) covers PSR; no separate PSR param

**optid's role regarding PSR** **[PROVEN design]**

optid does NOT enable/disable PSR (that's a kernel module parameter + driver policy).
optid **observes** PSR state and reports via `optctl explain display`:
```
display: eDP-1
  PSR2: active (link in standby) — saving ~0.8W
  VRR: 72 Hz (floor 48 Hz, ceiling 144 Hz)
  Backlight: 45% (DPST engaged, -12% reduction)
  DPMS: On
```

If PSR2 is frequently disengaging (high cursor_overlay_updates rate), optid can log a hint:
"PSR2 disengaged 47 times in last 60s — check compositor cursor animation settings."

### 1.6 VRR / Adaptive-Sync

**KMS VRR interface** **[PROVEN]**

`vrr_enabled` — DRM connector property, boolean (0/1). Set via `DRM_IOCTL_MODE_SETPROPERTY`
(KMS ioctl, requires DRM master). Compositor must own this; optid hints only.

eDP VRR floor: typically 40–48 Hz for laptop panels (VESA eDP spec minimum). Some panels
have 60 Hz minimum (usually higher-end panels with specific panel drivers). The floor is
in the panel's EDID under the Display Range Limits descriptor.

**Compositor VRR support (2024–2026):** **[PROVEN]**
- Mutter (GNOME Shell): VRR since 45.0 (2023); enabled via `org.gnome.mutter.experimental-features`
  `["variable-refresh-rate"]`
- KWin (KDE Plasma): VRR since Plasma 5.26 (2022); `kscreen` setting
- wlroots-based (Sway, Hyprland, etc.): VRR via `wlr_output.adaptive_sync_status`

**optid's hint transport** **[HYPOTHESIS]**

optid cannot call the VRR KMS ioctl. Two hint paths:
1. D-Bus signal `org.rush.Optid.VrrHint(u target_hz, s reason)` from optid to the bridge;
   bridge relays to the compositor via compositor-specific D-Bus API or config file.
2. JSON file at `/run/user/$UID/optid/hints.json` (inotify); bridge reads and applies.

Policy:
- workload class `idle` or `light-interactive-idle`: hint `target_hz=48` (minimum)
- workload class `latency-critical` or `interactive-game`: hint `target_hz=max` (ceiling)
- workload class `throughput` (compile): hint `target_hz=60` (save power, no active UI)

**VRR ramp latency** **[HYPOTHESIS]**

eDP VRR ramp from 48 Hz to 120 Hz happens within one frame period at 120 Hz (~8 ms). The
panel's Vmin/Vmax timing is managed by the hardware; no software latency is introduced by
enabling VRR. The **user-perceptible latency** is the response time from optid detecting
a new input event to the compositor applying the VRR hint — approximately 1 frame at the
current rate (48 Hz = 21 ms worst case).

### 1.7 Discrete Refresh Rate Switching (Non-VRR)

**Cost of modeset refresh rate change** **[PROVEN]**

Non-VRR refresh rate changes require a full KMS modeset, which involves blanking the display
for 50–200 ms with visible flicker. This is too disruptive for battery-saving use.

**Intel DRRS (Dynamic Refresh Rate Switching)** **[PROVEN]**

Some Intel platforms (Tiger Lake+) support seamless refresh rate switching via DC states on
eDP, implemented in `drivers/gpu/drm/i915/display/intel_seamless_drrs.c`. The switch is
seamless (no blanking) when supported. Capability is exposed via the connector's
`vrr_enabled` property (same as VRR) on kernels that have DRRS support.

**optid policy:** If seamless DRRS is supported (detected via connector property), hint rate
changes as with VRR. If not supported, do NOT recommend refresh rate changes — the modeset
cost (50–200 ms flicker) is worse than the power saving.

### 1.8 DPMS and Compositor Idle Policy

**DPMS KMS property** **[PROVEN]**

DRM connector property `DPMS` (values 0=On, 1=Standby, 2=Suspend, 3=Off). Set via
`DRM_IOCTL_MODE_SETPROPERTY`. Compositor-owned.

**optid hint:** D-Bus `org.rush.Optid.DpmsHint(u min_seconds, s reason)`:
- Class `idle` + battery < 20%: min_seconds = 60 (aggressive DPMS)
- Class `light` + battery: min_seconds = 120
- Class `interactive`: min_seconds = 300
- Class `latency-critical` (video call, game): min_seconds = 0 (never DPMS)
- Class `throughput` (compile, no user at keyboard): min_seconds = 30

The compositor's own idle settings (GNOME: `org.gnome.desktop.session.idle-delay`) are
overridden by the hint when optid is managing DPMS. The compositor applies
`max(user_preference, optid_hint_seconds)` to avoid being *more* aggressive than the user wants.

**Lid switch** **[PROVEN]**

`/proc/acpi/button/lid/LID/state` (or `LID0`) exposes current lid state. systemd-logind
generates a D-Bus signal `org.freedesktop.login1.Manager.LidClosed`. optid listens for this
signal (as root) and sets `DPMS=Off` via the bridge immediately on lid close, without waiting
for the idle timer.

### 1.9 Quantitative Energy Estimates

All numbers `[HYPOTHESIS]` until measured on reference hardware.

| Component | Estimate | Notes |
|-----------|---------|-------|
| LCD panel + backlight at 50% | 2–4 W | 14-16" panel |
| OLED panel at 50% brightness | 1.5–3 W | Lower at dark content |
| PSR2 engagement during static screen | -0.5 to -1.5 W | Link + controller savings |
| VRR at 48 Hz vs 120 Hz | -0.2 to -0.5 W | Panel refresh savings |
| DPST/ABM at level 2 | -0.3 to -0.8 W | Backlight reduction |
| Backlight at 30% vs 70% | -0.8 to -1.5 W | Linear with brightness |
| DPMS Off | -2 to -4 W | Full panel + link off |

* * *

## 2. Architecture — Design Decisions

### Decision 1: Bridge pattern reuse
**C — umbrella `optid-session-bridges` crate** with `focus`, `display`, `audio`, `gamemode`
modules as `systemd --user` services installed from one Rust crate. Reduces code duplication.

### Decision 2: Backlight write ownership
**A — optid system service writes `/sys/class/backlight/<bl>/brightness` directly** (root).
The bridge reads only (observes PSR/VRR state). The compositor is out of the loop for
backlight writes (it doesn't need to know; optid is the policy engine).

### Decision 3: PSR observability
**A — bridge reads `/sys/kernel/debug/dri/0/i915_edp_psr_status`** (with root or debug group).
optid (system service) never touches debugfs — that's the bridge's domain.
AMD: bridge reads `amdgpu_dm_psr_info`.

### Decision 4: VRR/DPMS hint transport
**A + B hybrid** — D-Bus signal for low-latency push + JSON file at
`/run/user/$UID/optid/display-hints.json` as fallback/cache. The JSON file allows any
compositor plugin to read state without D-Bus.

### Decision 5: DPST/ABM default policy
**B — on for `idle`/`light`/`interactive`, off for `latency-critical`/video** (detected via
fullscreen+audio state from 0005 bridge).

### Decision 6: PWM flicker floor
**A — minimum 30% brightness if HWID is marked `pwm_low_freq=true` in allowlist.**
Default: no floor (safe for DC-dimming panels).

* * *

## 4. Evidence Gaps

### 4.1 PSR2 Wake Latency on T14 Gen 4

```bash
sudo modprobe i915 enable_psr=2
# Watch PSR status while typing:
watch -n 0.5 'grep -i "standby\|active" /sys/kernel/debug/dri/0/i915_edp_psr_status'
# Use evtest to inject keypress; measure transition timing:
evtest /dev/input/event0 | head -1 &
# Timestamp keypress vs PSR-inactive transition
```

**Acceptance threshold:** < 10 ms from keypress to PSR inactive (below interactive floor).

### 4.2 VRR Ramp Latency on XPS 13 9320

```bash
# Enable VRR via Mutter experimental feature
gsettings set org.gnome.mutter experimental-features "['variable-refresh-rate']"
# Generate input event; measure frame timestamps:
sudo cat /sys/kernel/debug/dri/0/i915_display_info | grep -i vrr
```

**Acceptance threshold:** < 50 ms from hint signal to VRR rate change visible in vblank timestamps.

### 4.3 DPST Energy Saving on T14 Gen 4

```bash
# Baseline: DPST off, backlight 50%, static editor content
sudo sh -c 'echo 0 > /sys/kernel/debug/dri/0/i915_dpst_disable'  # enable DPST
# Wait 30s for DPST to engage, measure package power:
sudo turbostat --quiet --show PkgWatt,GFXWatt --interval 5 -n 12 | tee /tmp/dpst-on.txt
# Disable DPST:
sudo sh -c 'echo 1 > /sys/kernel/debug/dri/0/i915_dpst_disable'
sudo turbostat --quiet --show PkgWatt,GFXWatt --interval 5 -n 12 | tee /tmp/dpst-off.txt
```

**Acceptance threshold:** > 0.3 W package power reduction with DPST on; no perceptible quality loss.

### 4.4 PWM Frequency Detection on Reference Laptops

```bash
# Check sysfs for vendor-specific PWM attributes:
ls /sys/class/backlight/intel_backlight/
cat /sys/class/backlight/intel_backlight/type
# If "platform": may be PWM-based
# Measure with slow-mo video or photodiode at 10% brightness
```

**Acceptance threshold:** PWM frequency identified per reference laptop. If < 1 kHz:
add `pwm_low_freq=true` to allowlist entry and document the brightness floor.

* * *

## 5. Non-Goals

- **No direct KMS writes from optid.** Modesets are compositor-owned.
- **No HDR / mini-LED zone control.** Out of scope for v0.x.
- **No per-pixel backlight control.** DPST/ABM is the granularity, not per-zone.
- **No flicker-based energy saving.** PWM floor is enforced.
- **No competing auto-brightness daemon.** Cooperates with compositor via floor/preference split.
- **No PSR enable/disable from optid.** Kernel module parameter + driver policy only.
- **No panel overclocking.** Refresh rate stays within EDID-spec range.
- **No compositor-specific plugins.** Bridge speaks standard D-Bus / Wayland protocols.

* * *

## 6. WP Relationship Map

| Workplan / Doc | Relationship |
|----------------|-------------|
| **WP-N7 (panel side)** | Direct subject |
| **WP-N7 (dGPU side)** | Separate slot 0011, reuses this bridge pattern |
| **WP-N1** | Needs class detection for fullscreen/video/editor classification |
| **WP-N2** | PSR2 wake latency feeds into PM QoS floor |
| **ADR-0013** | Display state (PSR, VRR, DPMS) is signal; backlight target is policy |
| **0005 (focus-bridge)** | Reuses bridge pattern; same umbrella crate |
| **0002** | Freshens — display was left as gap |

* * *

## 7. Next Steps

### Immediate (no hardware needed)
- [ ] Draft `org.rush.Optid.DisplayHint` D-Bus interface XML
- [ ] Add `crates/optid-session-bridges/src/display.rs` skeleton
- [ ] Implement backlight device selection heuristic (§1.1)
- [ ] Implement `crates/optid/src/display.rs` policy module skeleton

### Short-term (needs hardware)
- [ ] Run §4.1 PSR2 wake latency on T14 Gen 4
- [ ] Run §4.2 VRR ramp latency on XPS 13
- [ ] Run §4.3 DPST energy saving measurement
- [ ] Run §4.4 PWM frequency on each reference laptop

### Medium-term
- [ ] Land `--display-bridge=bus` as default
- [ ] Promote research from WIP to Validated once §4.1–§4.4 closed on ≥ 3 reference laptops
- [ ] Wire multi-monitor support (per-connector hints)

* * *

## Appendix: Suggested Reading

### Kernel source
- `drivers/gpu/drm/i915/display/intel_psr.c` — PSR1/PSR2 implementation
- `drivers/gpu/drm/i915/display/intel_backlight.c`
- `drivers/gpu/drm/i915/display/intel_seamless_drrs.c`
- `drivers/gpu/drm/amd/display/amdgpu_dm/` — AMD display manager
- `drivers/acpi/video_detect.c` — backlight device selection
- `drivers/video/backlight/` — backlight core

### Documentation
- `Documentation/gpu/i915.rst` — PSR, DPST module params
- `Documentation/gpu/drm-kms.rst` — VRR, DPMS properties
- `Documentation/ABI/stable/sysfs-class-backlight`
- `Documentation/ABI/stable/sysfs-bus-iio`

### Prior art
- `gnome-settings-daemon` power plugin — backlight selection
- KDE `powerdevil` — similar display power policy
- `wlroots` `wlr_output` API — VRR enable

### Project-internal
- SPEC §3 (actuation rule), §4.1, §4.3, §6 WP-N7
- ADR-0013 (`docs/decisions/0013-detection-and-ml-boundary.md`)
- Research 0005 (focus-bridge, bridge pattern)
- Research 0002, 0003
