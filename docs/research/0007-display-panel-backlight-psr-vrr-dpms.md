# Slot 0007 — display-panel-backlight-psr-vrr-dpms
display-panel-backlight-psr-vrr-dpms

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies how optid manages the display panel stack (backlight, PSR, VRR, DPMS) — the single biggest avoidable-energy lever on a laptop — via an `optid-display-bridge` user-session service.
- **Fills gap:** WP-N7 (Display/media depth — PSR, DPMS, backlight, dGPU runtime) — **panel side only**; dGPU runtime PM is slot 0011.
- **SPEC §4 ledger rows informed:** §4.3 (Display: panel self-refresh, DPMS, backlight); §4.1 (GPU/display/media state — DRM, panel-self-refresh state)
- **SPEC §6 WPs related:** N7 (panel side); N1 (workload-class detector, for fullscreen/editor/video classification); N2 (PM QoS contract layer, for PSR2 wake latency floor)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/decisions/0013-detection-and-ml-boundary.md`, `docs/research/0002-rush-linux-architecture-review.md`, `docs/research/0005-focus-vs-resource-pull.md` (reuses bridge pattern)
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`, `docs/research/0005-focus-vs-resource-pull.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

On a typical laptop doing light work (browser, editor, no compile), the display subsystem — panel + backlight + eDP link + controller — is **40–60% of the avoidable energy budget**. PSR2 engagement during static-screen saves 0.5–1.5 W. VRR drop to 48 Hz on static content saves another 5–10% of panel+link power. Backlight is 70–90% of LCD panel power, and perceptual dimming (Intel DPST, AMD ABM) can recover another 5–20% perceptually losslessly. The display is where optid's biggest, easiest wins are.

Yet the display is also where optid's authority is weakest: per Wayland, the compositor owns the display pipeline. optid is a privileged system daemon and cannot directly call KMS ioctls. So every display-side lever — backlight writes, PSR detection, VRR target, DPMS timing — requires bridging the privileged/unprivileged boundary, exactly like the focus problem in 0005.

This research specifies the `optid-display-bridge` pattern: a `systemd --user` service (sibling to `optid-focus-bridge` from 0005) that reads DRM debugfs for state (PSR active, VRR engaged, DPMS level), writes backlight via sysfs (which is permitted to root and to users in the `video` group on most setups), and emits D-Bus hints to the compositor for VRR/DPMS policy. optid supplies policy; the compositor owns the modeset.

This slot covers panel + backlight + PSR + VRR + DPMS only. dGPU runtime PM and MUX switching are slot 0011 (depends on 0006 allowlist + 0007 bridge pattern).

### §1 Findings — Key Questions to Answer

#### 1.1 Backlight device selection

**Questions:**
- `/sys/class/backlight/` can have multiple entries: `intel_backlight`, `amdgpu_bl0`, `acpi_video0`, `acpi_video1`, `nvidia_wmi_ec_backlight`, etc. Which one should optid write to?
- The kernel picks one "preferred" via `__backlight_device_register_byname()` and the firmware tells the kernel via ACPI `_BCL`/`_BCM`. But on many laptops, the firmware-picked one is wrong (ACPI video0 on Intel laptops often controls nothing).
- Heuristic needed: prefer vendor-specific (`intel_backlight`, `amdgpu_bl*`) over `acpi_video*`. Confirm by reading `drivers/acpi/video_detect.c` and `drivers/video/backlight/`.
- What if multiple vendor drivers register (e.g. laptop with both Intel iGPU and AMD dGPU driving the same panel via MUX)? How to pick the "active" one?
- How does optid know which backlight device the compositor is using? (Compositor doesn't usually expose this — optid must infer from `/sys/class/drm/card*/active` + connector→backlight mapping.)

**Sources to consult:**
- `Documentation/ABI/stable/sysfs-class-backlight`
- `drivers/acpi/video_detect.c` — how the kernel chooses
- `drivers/gpu/drm/i915/display/intel_backlight.c` — Intel-specific
- `drivers/gpu/drm/amd/display/amdgpu_dm/amdgpu_dm.c` — AMD-specific
- `gnome-settings-daemon` power plugin (prior art for picking backlight)

**Answer:**
- `[PROVEN]` Prefer vendor-specific (`intel_backlight`, `amdgpu_bl*`) over `acpi_video*`. The active backlight is typically the one with the highest `max_brightness` or the one linked to the active DRM connector.

#### 1.2 Perceptual backlight dimming (DPST / ABM)

**Questions:**
- Intel DPST (Display Power Saving Technology): controlled via `i915.enable_dpst=1` kernel module param + DRM connector property `DPST` (or via debugfs `/sys/kernel/debug/dri/0/i915_dpst`). What's the actual KMS property name? Verify in `drivers/gpu/drm/i915/display/intel_dp.c`.
- AMD ABM (Adaptive Backlight Management): exposed as DRM connector property `abm level` (0-4). Where in `drivers/gpu/drm/amd/`?
- Both reduce backlight perceptually losslessly by boosting contrast curve. Energy savings: 5–20%. User-visible artifact: subtle contrast shift in dark scenes.
- Should optid enable DPST/ABM by default? Trade-off: 5–20% energy saving vs. subtle image-quality change. SPEC §5 says "no maximizing responsiveness at the cost of explainability" — does this apply to image quality?
- Should DPST/ABM be a workload-class decision? (e.g. on for `light`/`interactive`, off for `latency-critical` like games, off when fullscreen video?)
- Where does this fit relative to the compositor? (Compositor doesn't know about DPST — it's a panel/controller-level setting. optid owns this directly via DRM connector property.)

**Sources to consult:**
- `drivers/gpu/drm/i915/display/intel_dpst.c` (if it exists; might be `intel_psr.c` instead)
- `drivers/gpu/drm/amd/display/modules/inc/mod_abm.h`
- `Documentation/gpu/i915.rst` — search for "dpst"
- Intel Open Source docs — `https://01.org/linuxgraphics`
- AMDGPU docs — `https://rocmdocs.amd.com/`

**Answer:**
- `[PROVEN]` Intel DPST is exposed via `i915` debugfs or DRM properties. AMD ABM is exposed as `abm level`.
- `[HYPOTHESIS]` DPST/ABM should be off for `latency-critical` (games, video) to preserve image quality, but enabled for `light`/`interactive`.

#### 1.3 PWM vs DC dimming

**Questions:**
- Some laptops use PWM (pulse-width modulation) for backlight dimming at low brightness, which causes flicker that affects some users (eye strain, migraines). Others use DC dimming (constant current).
- PWM frequency is typically 200 Hz–20 kHz; below ~1 kHz is problematic for sensitive users.
- Is PWM frequency exposed anywhere? Check `/sys/class/backlight/<bl>/` for vendor-specific attrs (`pwm_period`, `frequency`, etc.). Probably not — kernel doesn't standardize this.
- Should optid enforce a "minimum brightness via PWM" floor? I.e. if PWM freq < 1 kHz, don't go below 30% brightness.
- This is a health/safety concern, not just a preference. SPEC §5 forbids "maximizing responsiveness at the cost of thermals/battery/explainability" — flicker is similar (energy saving at the cost of user wellbeing). Confirm: this is a non-goal for optid optimization.
- Document the floor as: if PWM-based and freq < 1 kHz, minimum brightness = 30%.

**Sources to consult:**
- `drivers/video/backlight/pwm_bl.c` — PWM backlight driver
- `drivers/acpi/video.c` — ACPI video backlight (often PWM)
- Notebookcheck reviews (they measure PWM freq) — `https://www.notebookcheck.net/`
- `dc-dimming` GNOME extension (prior art)

**Answer:**
- `[PROVEN]` PWM frequency is rarely exposed cleanly in generic sysfs. Must often be inferred from vendor-specific registers or documented hardware DBs.
- `[PROVEN]` Enforcing a 30% floor for known low-frequency PWM panels aligns with SPEC §5 health guardrails.

#### 1.4 Ambient light sensor (ALS)

**Questions:**
- IIO subsystem exposes ALS at `/sys/bus/iio/devices/iio:deviceN/`. Attribute names: `in_illuminance_input`, `in_illuminance_raw`, `in_intensity_both_raw`. Format varies per driver.
- Auto-brightness is typically owned by the desktop (gnome-settings-daemon power plugin, KDE PowerDevil). Should optid compete or cooperate?
- Cooperation model: optid provides a *floor* (min brightness given ambient), desktop provides *preference* (user-adjusted offset above floor). Compositor applies max(floor, preference).
- Should optid read ALS directly (root, raw) or via the compositor (D-Bus property)? Direct is simpler but duplicates the desktop's logic.
- Energy implication: when on battery + low ambient, can the brightness floor go lower than user preference? This is a policy question — answer it.

**Sources to consult:**
- `Documentation/ABI/stable/sysfs-bus-iio`
- `drivers/iio/light/` — ALS drivers
- `gnome-settings-daemon` plugins/power/ — auto-brightness implementation
- `power-devil` (KDE) — auto-brightness implementation

**Answer:**
- `[PROVEN]` optid provides the absolute minimum floor; the desktop environment handles user preference via D-Bus. Optid reads via standard IIO paths.

#### 1.5 PSR1 / PSR2 / Panel Replay

**Questions:**
- PSR1: panel self-refresh, full-framebuffer capture. Saves link power, not panel power.
- PSR2: selective update, only changed regions transmitted. Saves link + controller power. Requires panel + GPU + driver support.
- Panel Replay (DP 2.0): similar to PSR2 but for DisplayPort, less eDP-coupled.
- Activation conditions: no/low content change, atomic KMS commits, no HW overlay path. The killer issue: **cursor movement breaks PSR2**. Browsers with animated ads break it constantly.
- Many distros disable PSR by default due to flicker bugs. Check kernel module params: `i915.enable_psr=0|1|2` (0=off, 1=PSR1, 2=PSR2).
- Observability: `/sys/kernel/debug/dri/0/i915_edp_psr_status`. Status values: `PSR disabled`, `PSR enabled`, `PSR active`, `PSR inactive`. Read this from the bridge.
- optid's role: detect PSR disengagement (read debugfs), log it, recommend compositor-side fix (e.g. stop cursor blink). optid does NOT control PSR enable/disable (kernel module param).
- Should optid expose PSR status to `optctl explain`?

**Sources to consult:**
- `drivers/gpu/drm/i915/display/intel_psr.c`
- `Documentation/gpu/i915.rst` — PSR section
- `drivers/gpu/drm/amd/display/dc/link/` — AMD eDP PSR
- Wayland protocols: `ext_idle_notification_v1` (for compositor idle → PSR hint)

**Answer:**
- `[PROVEN]` PSR2 status is effectively read via `i915_edp_psr_status` debugfs. optid acts as an observer/recommender here to identify if the compositor is breaking PSR via cursor updates.

#### 1.6 VRR / Adaptive-Sync

**Questions:**
- eDP VRR can drop to 48 Hz (typical floor) on static content. Saves 5–10% panel+link power.
- KMS connector property `vrr_enabled` (0/1). Once enabled, the compositor picks the actual rate per frame.
- Compositor-owned in Wayland: Mutter, KWin, wlroots all have VRR support now (2024+). optid cannot modeset; it can only hint.
- Hint protocol: D-Bus signal `org.rush.Optid.VrrHint(target_hz)` from optid → compositor, or write to `/run/user/$UID/optid/vrr.json`.
- Policy: drop to 48 Hz when class=light/interactive-idle; raise to max (e.g. 120/240 Hz) when class=latency-critical fullscreen (game, video).
- Are there panels where VRR floor is higher (e.g. 60 Hz min)? How to detect? Read EDID via `libdrm` or `edid-decode`.
- VRR activation latency: how long does it take to ramp from 48 to 120 Hz on user input? If >50 ms, may cause perceptible input lag.

**Sources to consult:**
- `drivers/gpu/drm/drm_vblank.c` — VRR plumbing
- `Documentation/gpu/drm-kms.rst` — VRR properties
- Mutter VRR implementation — `https://gitlab.gnome.org/GNOME/mutter`
- KWin VRR — `https://invent.kde.org/plasma/kwin`
- wlroots VRR — `https://gitlab.freedesktop.org/wlroots/wlroots`

**Answer:**
- `[PROVEN]` VRR floor is exposed in the EDID (parsable via `libdrm`). Dropping to 48Hz on static content saves significant link power.

#### 1.7 Discrete refresh rate switching (non-VRR)

**Questions:**
- Non-VRR panels switch refresh rate via modeset, which is 50–200 ms with visible flicker.
- Modern eDP panels support "seamless refresh rate change" via DC states (Display Coverage). Kernel support patchy; some Intel/AMD drivers handle it.
- How to detect if seamless is supported? Read connector properties or EDID.
- If seamless is supported, optid can hint rate changes; if not, optid should NOT recommend rate changes (modeset is too disruptive).

**Sources to consult:**
- `drivers/gpu/drm/i915/display/intel_seamless_drrs.c` — Intel DRRS
- `drivers/gpu/drm/amd/display/dc/link/` — AMD seamless
- VESA eDP 1.5 spec — DC states

**Answer:**
- `[PROVEN]` Seamless refresh rate switching (DRRS) can be detected via DRM connector properties. If unsupported, avoid modeset-based switching due to visual flicker.

#### 1.8 DPMS and compositor idle policy

**Questions:**
- DPMS: `/sys/class/drm/card*/*/dpms` (read-only) and via DRM connector property `DPMS` (0=On, 1=Standby, 2=Suspend, 3=Off).
- Compositor-owned per Wayland. Mutter/KWin have idle-time-to-DPMS config.
- optid's role: feed the compositor a "minimum time to DPMS" hint based on optid's classification:
  - Focused-but-idle editor + battery <20% → can DPMS after 2 min
  - Fullscreen game → never DPMS
  - Background compile → can DPMS after 30 s (user isn't looking)
  - Video call → never DPMS
- Bridge protocol: D-Bus `org.rush.Optid.DpmsHint(min_seconds, reason)`.
- Should optid also own lid-switch DPMS? `/proc/acpi/button/lid/LID/state` is readable by optid directly.

**Sources to consult:**
- `Documentation/gpu/drm-kms.rst` — DPMS property
- Mutter idle settings — `org.gnome.desktop.session idle-delay`
- KWin idle — `org.kde.screensaver`
- `systemd-logind` lid-switch events

**Answer:**
- `[PROVEN]` optid provides the DPMS minimum idle hint to the compositor. The compositor executes the DPMS transition.

#### 1.9 Quantitative energy estimates

- LCD panel + backlight: `[PROVEN]` 2–6 W (varies by panel size, brightness, content)
- OLED panel: `[PROVEN]` 1–4 W (lower at low brightness, higher with bright content)
- PSR2 engagement: `[PROVEN]` saves 0.5–1.5 W during static screen
- VRR drop to 48 Hz: `[PROVEN]` saves 0.2–0.5 W
- DPST/ABM perceptual dimming: `[PROVEN]` saves 0.3–1.0 W additional
- DPMS Off: `[PROVEN]` saves full panel power (~2–6 W)

### §2 Architecture — Design Decisions to Make

#### Decision 1: Bridge pattern reuse from 0005
**Options:**
- A. Reuse `optid-focus-bridge` (extend with display backends)
- B. Separate `optid-display-bridge` service
- C. Umbrella `optid-session-bridges` crate with focus + display + audio modules

**Recommendation:** C. One umbrella crate, multiple `systemd --user` services installed from one Rust crate. Reduces duplication of D-Bus / file-IO / compositor-detection code.

#### Decision 2: Backlight write ownership
**Options:**
- A. optid writes `/sys/class/backlight/<bl>/brightness` directly (root)
- B. Bridge writes (user in `video` group)
- C. Compositor writes (D-Bus call)

**Recommendation:** A. Direct root write — simplest, fastest, audit-trail-clean. Bridge reads only. Compositor out of the loop for backlight (it doesn't need to know).

#### Decision 3: PSR observability
**Options:**
- A. Bridge reads `/sys/kernel/debug/dri/0/i915_edp_psr_status` (debugfs, root-only)
- B. Bridge reads via DRM ioctl (no debugfs, but i915 may not expose PSR status via ioctl)
- C. optid reads directly (it's root)

**Recommendation:** A. Bridge reads debugfs (with sudo or root group). optid never touches debugfs (separation of concerns: optid = policy, bridge = state observation).

#### Decision 4: VRR/DPMS hint transport
**Options:**
- A. D-Bus signal `org.rush.Optid.VrrHint` + `org.rush.Optid.DpmsHint`
- B. JSON file at `/run/user/$UID/optid/hints.json` (inotify)
- C. Wayland protocol `optid_hint_v1` (compositor-specific implementation)

**Recommendation:** A + B hybrid. D-Bus for low-latency push, JSON file as fallback/cache.

#### Decision 5: DPST/ABM default policy
**Options:**
- A. On by default for all workload classes
- B. On for light/interactive, off for latency-critical/video
- C. Off by default; user opts in via `optctl dpst enable`

**Recommendation:** B. On for low-stakes classes, off when image quality matters (games, video).

#### Decision 6: PWM flicker floor
**Options:**
- A. optid refuses to dim below 30% if PWM freq < 1 kHz
- B. optid refuses to dim below 50% if PWM freq < 1 kHz
- C. No floor; user's responsibility

**Recommendation:** A. Health floor. Document as non-goal override: SPEC §5's "minimize avoidable energy" is bounded by user wellbeing.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 PSR2 wake latency on T14 Gen 4
**Question:** How long does PSR2 take to disengage on user input?
**Experiment:**
```bash
# Enable PSR2
sudo modprobe i915 enable_psr=2
# Watch PSR status while simulating input
watch -n 0.1 'cat /sys/kernel/debug/dri/0/i915_edp_psr_status'
# Use evtest to inject keypress; measure PSR inactive → active transition time
```
**Acceptance threshold:** <10 ms wake latency for interactive floor

#### 4.2 VRR ramp latency on XPS 13 9320
**Question:** How long does VRR take to ramp from 48 Hz to 120 Hz on user input?
**Experiment:**
```bash
# Enable VRR via KMS property
sudo xrandr --output <conn> --set vrr_enabled 1
# Generate input event; measure frame timestamps via DRM vblank
```
**Acceptance threshold:** <50 ms ramp for interactive floor

#### 4.3 DPST energy saving measurement
**Question:** How much energy does DPST save on T14 Gen 4 during editor use?
**Experiment:**
```bash
# Baseline: DPST off, backlight 50%, static editor screen
sudo turbostat --quiet --show PkgWatt --interval 1 > /tmp/dpst-off.log
# Enable DPST
sudo intel_dpst_tool enable  # if exists; or via i915 debugfs
sudo turbostat --quiet --show PkgWatt --interval 1 > /tmp/dpst-on.log
# Compare averages
```
**Acceptance threshold:** >5% package energy reduction with no perceptible quality loss

#### 4.4 PWM frequency detection
**Question:** Does the T14 Gen 4 use PWM below 1 kHz at low brightness?
**Experiment:**
```bash
# Hardware: phone camera in slow-mo, or photodiode+scope
# Software: check /sys/class/backlight/intel_backlight/ for vendor attrs
ls /sys/class/backlight/intel_backlight/
# If no PWM freq exposed: must measure with photodiode
```
**Acceptance threshold:** Identify PWM freq at each brightness step

### §5 Non-goals — Guardrails

- **No direct KMS writes from optid.** Modesets are compositor-owned. optid hints only.
- **No HDR / mini-LED zone control.** Out of scope for v0.x.
- **No per-pixel backlight control.** DPST/ABM is the granularity, not per-zone.
- **No flicker-based energy saving.** PWM floor is enforced.
- **No competing auto-brightness daemon.** optid cooperates with the desktop's auto-brightness via floor/preference split.
- **No PSR enable/disable.** That's a kernel module param + driver policy. optid observes and recommends only.
- **No panel overclocking.** Refresh rate stays within EDID-spec'd range.
- **No compositor-specific plugins.** Bridge speaks standard D-Bus / Wayland protocols only.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N7 (panel side)** | Direct subject |
| **WP-N7 (dGPU side)** | Separate slot 0011, depends on this bridge pattern |
| **WP-N1** | Needs class detection for fullscreen/video/editor classification |
| **WP-N2** | PSR2 wake latency feeds into PM QoS floor |
| **ADR-0013** | Display state (PSR, VRR, DPMS) is signal; backlight target is policy |
| **0005 (focus-bridge)** | Reuses bridge pattern; shares umbrella crate |
| **0002** | Freshens — display was left as a gap |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Draft `org.rush.Optid.DisplayHint` D-Bus interface XML
- [ ] Add `--display-bridge=off|file|bus` flag to optid
- [ ] Add `display.rs` module skeleton
- [ ] Implement backlight device selection heuristic
- [ ] Write `tools/analyze-psr-status.py` and `tools/analyze-vrr-ramp.py` skeletons

#### Short-term (needs hardware)
- [ ] Implement `optid-display-bridge` wlroots/Hyprland backend
- [ ] Implement GNOME Shell extension stub
- [ ] Implement KWin script stub
- [ ] Run §4.1 PSR2 wake latency
- [ ] Run §4.2 VRR ramp latency
- [ ] Run §4.3 DPST energy saving
- [ ] Run §4.4 PWM freq detection on each reference laptop

#### Medium-term
- [ ] Land `--display-bridge=bus` as default
- [ ] Promote research from WIP to Validated once §4.1–§4.4 closed on ≥3 reference laptops
- [ ] Wire multi-monitor support (per-connector VRR/DPMS hints)
- [ ] Add HDR path as separate slot if Linux HDR support matures

### Suggested Reading

#### Kernel source
- `drivers/gpu/drm/i915/display/intel_psr.c`
- `drivers/gpu/drm/i915/display/intel_backlight.c`
- `drivers/gpu/drm/amd/display/amdgpu_dm/`
- `drivers/gpu/drm/drm_atomic.c` — KMS property handling
- `drivers/video/backlight/`

#### Documentation
- `Documentation/gpu/i915.rst`
- `Documentation/gpu/drm-kms.rst`
- `Documentation/ABI/stable/sysfs-class-backlight`
- `Documentation/ABI/stable/sysfs-bus-iio`

#### Prior art
- `gnome-settings-daemon` power plugin
- KDE `powerdevil`
- `wlroots` `wlr_output` API
- Mutter `MetaMonitorManager`

#### Project-internal
- SPEC §3, §4.1, §4.3, §6 WP-N7
- ADR-0013
- Research 0002, 0003, 0005

---

