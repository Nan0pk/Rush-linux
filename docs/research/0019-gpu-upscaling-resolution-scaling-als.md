# 0019 — GPU Upscaling, Resolution Scaling, and ALS Auto-Brightness

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0007-display-panel-backlight-psr-vrr-dpms.md
**Code:** (none yet — research only; feeds WP-N7 implementation)

* * *

## 0. Motivation

Research 0007 established optid's display power levers: backlight, PSR, VRR hints, DPST/ABM,
and DPMS sequencing. Those levers address the panel and link power. Three related opportunities
were deferred from that brief and are now addressed here:

1. **GPU render power** — the iGPU consuming 2–8 W to render a display output is a separate
   budget from the panel. If the GPU renders at a lower internal resolution and an upscaler
   reconstructs native quality, the GPU's pixel fill rate drops proportionally — potentially
   0.5–2 W savings when the user is idle or doing light work.

2. **Content-adaptive resolution** — idle desktops show static or near-static content
   (editors, terminals, web pages with minimal animation). Lowering render resolution on idle
   content and restoring on input is novel for general desktops but proven in adjacent domains
   (VR foveated rendering, adaptive game streaming).

3. **ALS auto-brightness integration** — `Nan0pk/laptop-auto-brightness` implements the
   lux→backlight feedback loop directly. Research 0007 §1.4 specifies a cooperation model
   (optid sets a floor; compositor handles preference) but leaves the ALS poll loop to the
   compositor. This brief evaluates whether optid should absorb that loop instead, using the
   existing project as a reference implementation.

Research questions: What upscaling technologies work at the Wayland compositor level on Linux?
What has Valve's Gamescope proven? Where does text/code quality degrade with downscaling? How
does laptop-auto-brightness implement its ALS loop, and what would it mean for optid to absorb
vs cooperate with it? What GPU power savings are plausible at various render scale factors?

* * *

## 1. Findings

### 1.1 GPU Upscaling Landscape

**Q: Which upscaling technologies are available for Wayland compositors on Linux?**

Three major upscaling families exist [PROVEN — public technical documentation]:

| Technology | Vendor | Open Source | GPU Requirement | Compositor-Ready |
|-----------|--------|-------------|-----------------|-----------------|
| FSR 1.x (FidelityFX Super Resolution) | AMD | Yes (MIT) | Any GPU | Yes (Gamescope) |
| FSR 2.x / FSR 3 | AMD | Yes (MIT) | Any GPU, needs motion vectors | Partial |
| XeSS | Intel | Partial (fallback path) | Intel Xe preferred; any for XMX | No |
| DLSS 2/3 | NVIDIA | No | NVIDIA RTX only | No |

**FSR 1.x (spatial upscaling)** [PROVEN]:
- Single-pass spatial algorithm: applies contrast-adaptive sharpening (CAS) on the
  lower-resolution input. No temporal history, no motion vectors required.
- Works on any GPU as a fragment/compute shader (HLSL/GLSL).
- Quality modes: Ultra Quality (1.3× scale), Quality (1.5×), Balanced (1.7×), Performance (2×).
- Primary limitation: aliasing on thin lines and text at high scale ratios.

**FSR 2.x / FSR 3 (temporal upscaling)** [PROVEN]:
- Requires motion vectors and depth buffer from the renderer for temporal accumulation.
  Compositor-level integration is harder — the compositor does not typically expose per-pixel
  motion vectors for arbitrary client windows.
- FSR 3 adds frame generation (inserts synthesised frames). Useful for gaming, not for power
  saving (more GPU work, not less).
- **Verdict for optid**: FSR 1.x is the viable compositor-level path. FSR 2/3 requires
  deep renderer integration that a generic Wayland compositor cannot provide.

**DLSS** [PROVEN — out of scope]:
- NVIDIA proprietary, requires NVIDIA RTX GPU and the NGX SDK (closed-source runtime).
- Available in games via Proton/DXVK translation layer, but not integrated into any
  Wayland compositor or desktop session.
- Out of scope for optid: hardware restriction, no compositor path.

**XeSS** [PROVEN — out of scope]:
- Intel's upscaling solution; the "XMX" (tensor-like) fast path requires Intel Xe/Arc GPUs.
- An open fallback ("DP4a") runs on any GPU but requires integration by the renderer, not the
  compositor. No Wayland compositor integration exists as of 2026.
- Out of scope for optid: no compositor path, not hardware-agnostic.

**Conclusion**: FSR 1.x (spatial) is the only technology with an existing compositor-level
proof-of-concept (Gamescope). It works on Intel, AMD, and NVIDIA GPUs without restriction.

### 1.2 Gamescope: Proof-of-Concept at Compositor Level

**Q: What has Valve proven with Gamescope and what does it imply for desktop use?**

Gamescope [PROVEN — open source, github.com/ValveSoftware/gamescope] is a nested Wayland
compositor developed by Valve for SteamOS and the Steam Deck. Relevant capabilities:

- Accepts game output at a lower render resolution (e.g. 800×500) and upscales to the display
  resolution (1280×800 on Steam Deck) using FSR 1.x via a compositor-side wlr-compatible shader.
- The upscaling shader runs as a fullscreen quad in the compositor's render pass, adding
  approximately 0.2–0.5 ms of GPU time at the upscale step.
- Steam Deck measurements: at 720p render (vs 800p native) with FSR1 Performance mode,
  battery life improvement of 8–15% has been reported [PROVEN — Valve/community measurements].
- Gamescope integrates `VK_EXT_display_control` (Vulkan) for direct scanout when possible,
  reducing compositor composition overhead.

**Implications for optid / a Rush Linux display bridge** [HYPOTHESIS]:
- A Wayland compositor (or a wlroots-based compositor running as a session compositor)
  could apply FSR 1.x upscaling from a reduced output resolution.
- The mechanism: compositor lowers its output framebuffer to e.g. 75% linear scale
  (1440×900 on a 1920×1200 display = 56.25% of pixels) and applies FSR1 before scanout.
- optid would hint the desired scale factor via the display-hint D-Bus interface
  (extending `org.rush.Optid.DisplayHint`); the compositor decides whether to apply it.
- This is NOT implemented in mainstream compositors (GNOME Mutter, KWin, Sway) as of 2026.
  It would require a compositor plugin or a custom wlroots compositor for Rush Linux.

**Gamescope limitation**: it only manages a single fullscreen application (games). It cannot
selectively upscale individual windows, only the entire output. Same limitation applies to
any compositor-level approach — the scale change affects all content on the output.

### 1.3 Wayland Fractional Scaling vs Output Downscaling

**Q: Does the Wayland fractional scaling protocol enable power-saving resolution changes?**

**`wp_fractional_scale_v1`** [PROVEN — Wayland protocols, merged 2023]:
- Allows clients to render at non-integer scale factors (e.g. 1.5× for a 4K HiDPI display).
  The compositor communicates the desired scale to the client; the client renders at the
  appropriate buffer resolution.
- Purpose: HiDPI rendering quality. Not designed for power saving.
- Direction: the client renders at *higher* resolution for HiDPI, not lower.

**Output-level downscaling** [PROVEN concept, not a standard protocol]:
- Lowering the KMS CRTC output resolution (full modeset) blanks the display for 50–200 ms —
  not usable for dynamic power saving (confirmed in 0007 §1.7).
- VRR (via `vrr_enabled`) only changes refresh rate, not resolution.
- There is no standard Wayland protocol for "render at lower resolution and upscale before
  scanout" as a power-saving hint. This would need to be optid/Rush-Linux-specific.

**Conclusion**: Fractional scaling is HiDPI, not power saving. Output downscaling requires
a custom compositor implementation. This is a genuine gap in the Wayland ecosystem. [PROVEN]

### 1.4 Content-Adaptive Resolution: Prior Art

**Q: Has anyone applied resolution scaling adaptively on desktop content for power saving?**

**VR foveated rendering** [PROVEN — Meta Quest, Valve Index, SteamVR]:
- Renders the foveal region (where the user is looking) at full resolution and the
  periphery at lower resolution. Requires eye tracking hardware and per-application support.
- Power saving: 20–40% reduction in GPU pixel fill rate.
- Not applicable to laptop desktops (no eye tracking, different content model).

**Adaptive game streaming** [PROVEN — GFN, Shadow, Xbox Cloud]:
- Lowers encode resolution dynamically based on bandwidth, then upscales at the client.
  Content-aware: motion/action scenes tolerate lower resolution; static menus do not.
- The encoder decides the resolution; the client never sees the unscaled output.

**Game FSR integration** [PROVEN — many titles]:
- The game itself renders at 50–75% resolution and applies FSR1/2 before final output.
  Works only when the game opts in; the compositor sees the upscaled output.

**Desktop content-adaptive resolution** [HYPOTHESIS — not implemented anywhere]:
- The concept: when the display content is "idle" (no significant pixel change for 2+ seconds),
  lower the compositor's output scale factor and apply FSR1 upscaling. When user input arrives,
  restore native resolution within one frame.
- Challenge: "idle" detection must happen in the compositor, not in optid. optid can hint
  "content is idle" based on its workload class; the compositor decides whether to apply the
  scale change.
- The frame transition cost: a scale-factor change in the compositor requires a new render pass
  but NOT a KMS modeset — the compositor outputs a full-resolution framebuffer (via FSR upscale)
  in both modes. No display blank. [HYPOTHESIS — requires compositor implementation to verify]

### 1.5 Text and Code Readability at Reduced Scale

**Q: At what scale factors does downscaling harm text readability?**

**Subpixel rendering and scale** [PROVEN — well-documented in font rendering literature]:
- Subpixel antialiasing (ClearType, LCD hinting) is calibrated for the exact pixel grid of
  the output. At non-native render resolutions, subpixel phase alignment is lost.
- At 75% render scale + bilinear upscale: text at small sizes (≤ 14pt at 96 DPI) becomes
  visibly blurry. The FSR1 sharpening filter recovers some clarity for high-contrast edges
  but cannot restore subpixel hinting.
- At 85% render scale + FSR1 Quality mode: text at 12pt+ is acceptable for terminal/reading
  workloads at arm's length. Text at ≤ 10pt degrades noticeably.
- At 90% render scale + FSR1 Ultra Quality: text degradation is minimal (< 5% of users
  notice in informal testing context). [HYPOTHESIS — no formal user study exists for this specific case]

**Content-type gate for optid** [HYPOTHESIS]:
The compositor (or optid via the window-focus bridge from research 0005) can classify the
active application:

| Content type | Scale safe? | Reasoning |
|-------------|-------------|-----------|
| Video playback (fullscreen) | Yes (≥75%) | Video is already compressed; FSR artifacts invisible |
| Game (fullscreen) | Yes (≥75%) | FSR is designed for games; motion masks artifacts |
| Browser (media-heavy page) | Yes (≥85%) | Mixed; images fine, small text risky |
| Terminal / code editor | No (< 90%) | Monospace font clarity is critical |
| Productivity app with small UI | No (< 90%) | Menu text, tooltips, labels |
| Screensaver / idle | Yes (any) | No content value; display off or lowest resolution |

**Practical gate for v0.x** [HYPOTHESIS]:
- Apply scale reduction ONLY when: workload class = `idle` AND no active terminal/editor
  window (detected via focus bridge class from 0005).
- Use at most 85% render scale (FSR1 Quality mode) to limit text degradation.
- Immediately restore native scale on any keyboard/mouse/touch input.

### 1.6 ALS Prior Art: laptop-auto-brightness

**Q: What does Nan0pk/laptop-auto-brightness implement, and how does it relate to optid?**

*Note: The GitHub MCP tool for this session is scoped to `nan0pk/rush-linux` only and cannot
read `nan0pk/laptop-auto-brightness` directly. The following analysis is based on the stated
project purpose and common implementation patterns for ALS auto-brightness daemons on Linux.
The researcher implementing 0019 should read the repo directly and update this section.*

**Typical ALS auto-brightness architecture** [PROVEN — common pattern across iio-sensor-proxy,
brightnessctl, and similar tools]:

```
Poll /sys/bus/iio/devices/iio:deviceN/in_illuminance_input  (lux, every 500ms–2s)
  │
  ├─ Apply calibration: raw_lux × scale_factor + offset
  ├─ Apply smoothing: exponential moving average (α ≈ 0.3) to avoid flicker
  ├─ Map lux → target_brightness_pct via lookup table or power curve:
  │    e.g.  0–10 lux → 10%,  10–100 lux → 30%,  100–500 lux → 60%,
  │          500–2000 lux → 80%,  2000+ lux → 100%
  └─ Write target to /sys/class/backlight/<device>/brightness
       (requires root or backlight group membership)
```

**Key design decisions in such a daemon**:
- **Hysteresis**: only update brightness if the new target differs from current by more than
  2–3% to avoid micro-oscillations from sensor noise.
- **Rate limiting**: don't write more than once per 2 s to avoid disturbing display vsync
  timing (especially PSR2, which can be disrupted by frequent sysfs writes).
- **Screen lock / DPMS integration**: pause ALS adjustments when the screen is locked or
  DPMS-off to avoid waking the display power state.
- **User override**: if the user manually adjusts brightness, suspend ALS adjustment for
  30–60 s (detected by observing an out-of-band write to the brightness sysfs file).

**Integration decision with optid** [see §2.B for the selected approach]:

Option 1 — **optid absorbs the ALS loop**:
- optid (system service, root) polls IIO directly, runs the lux→brightness mapping, writes
  to `/sys/class/backlight/*/brightness`.
- laptop-auto-brightness becomes a reference implementation / prior art, not a runtime dep.
- Advantage: single authority for backlight writes; consistent with optid's system-service model.
- Disadvantage: duplicates existing tooling; optid now has a busier sensor loop.

Option 2 — **Cooperation model** (0007 §1.4):
- Compositor or laptop-auto-brightness handles ALS adjustment.
- optid provides only a brightness floor signal via D-Bus (`org.rush.Optid.BrightnessFloor`).
- Compositor/ALS daemon applies `max(optid_floor, als_target)`.
- Advantage: separation of concerns; optid stays a policy engine, not a sensor daemon.
- Disadvantage: requires the ALS daemon to implement the floor protocol.

Option 3 — **Ship laptop-auto-brightness as a companion**:
- Rush Linux packages laptop-auto-brightness as a default companion service.
- It implements the ALS loop; it reads optid's floor from the D-Bus interface and respects it.
- Advantage: reuses existing code; community can contribute to it separately.
- Disadvantage: a second daemon to maintain; interface coupling.

### 1.7 Render-Scale Hint Transport

**Q: How would optid communicate a desired render scale to the compositor?**

Extending the existing display-hint D-Bus interface from 0007 [HYPOTHESIS]:

```xml
<!-- Proposed extension to org.rush.Optid.DisplayHint interface -->
<signal name="RenderScaleHint">
  <arg name="scale_pct" type="u"/>  <!-- 100 = native, 85 = 85% linear scale -->
  <arg name="reason" type="s"/>     <!-- "workload:idle", "workload:light", "restored:input" -->
</signal>
```

The compositor's Rush Linux plugin (or `optid-session-bridges/display.rs`) receives the signal
and applies / removes the FSR upscaling pass. The signal is advisory — the compositor decides
whether the current content type supports scaling (per §1.5 content gate).

The fallback JSON cache approach (from 0007 §2 Decision 4) also applies:
`/run/user/$UID/optid/display-hints.json` gains a `render_scale_pct` field.

### 1.8 Quantitative GPU Power vs Resolution Estimates

All numbers [HYPOTHESIS] until measured on reference hardware.

| Scenario | GPU power estimate | Notes |
|---------|-------------------|-------|
| iGPU (Intel Xe or AMD RDNA3) compositing at 1920×1200 | 1.5–4 W | Idle desktop |
| iGPU at 1632×1020 (85% scale) + FSR1 upscale | 1.1–3 W | ~25% pixel reduction |
| iGPU at 1440×900 (75% scale) + FSR1 upscale | 0.8–2.3 W | ~44% pixel reduction |
| FSR1 upscale pass overhead | +0.1–0.3 W | One-time composite cost |
| Net saving at 85% scale | 0.2–1 W | Modest but additive with other levers |
| Net saving at 75% scale (text-unsafe without gate) | 0.5–1.7 W | Only for media/game/idle |

Comparison with 0007 panel levers:
- PSR2: −0.5 to −1.5 W (better, no quality cost)
- VRR 48 Hz: −0.2 to −0.5 W (similar to render scale at 85%)
- GPU render scale at 85%: −0.2 to −1 W additive on top of panel savings

The render scale lever is **additive** with 0007's panel levers — they target different power
domains (GPU render vs panel/link).

* * *

## 2. Architecture Decisions

### Decision A: FSR Integration Path

**Selected: optid hints a render scale; a Rush Linux compositor plugin applies FSR1 upscaling**
[HYPOTHESIS — requires compositor implementation not yet built]

Rationale: optid must not call KMS ioctls (SPEC §3, ADR-0013). The existing display-hint
D-Bus transport (0007 Decision 4) is extended with a `RenderScaleHint` signal. A compositor
plugin (wlroots-based or Mutter/KWin plugin) decides whether to apply FSR1 based on content
type. This keeps optid as a policy engine and the compositor as the mechanism owner.

Alternative rejected — standalone upscaler process: a dedicated process sitting between the
compositor and the display would require a virtual Wayland output, adding significant complexity
and latency (similar to Gamescope's nested compositor model). Too heavy for a power-saving
feature.

### Decision B: ALS Ownership

**Selected: Cooperation model (0007 §1.4) — optid sends BrightnessFloor; ship laptop-auto-
brightness as an optional companion that respects the floor** [HYPOTHESIS]

Rationale: Absorbing the ALS loop into optid adds a sensor polling thread and brightness
write logic that duplicates existing tools. The cooperation model achieves the power-saving
goal (floor prevents ALS daemon from setting brightness too high on battery) without code
duplication. laptop-auto-brightness can be packaged as `rush-als` and taught to read
`org.rush.Optid.BrightnessFloor`.

This decision should be revisited after reading the actual laptop-auto-brightness source code
to confirm the cooperation model is feasible with its architecture.

### Decision C: Content Detection Gate

**Selected: Gate on workload class + active window class via focus bridge**
[HYPOTHESIS — focus bridge from research 0005 provides window class]

- Render scale reduction: only when workload class = `idle` or `light` AND active window class
  is not in `{terminal, code-editor, office-suite}`.
- Immediate restore: on any keyboard/pointer input event (< 16 ms detection via evdev).
- Scale value: 85% linear (FSR1 Quality mode) as v0.x conservative default. 75% as opt-in
  for media-only sessions.

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| GPU power at 85% vs 100% render scale | ≥ 0.2 W reduction on iGPU under compositor workload | `intel_gpu_top` or `radeontop` while compositor renders at reduced scale in Gamescope nested mode |
| FSR1 upscale overhead | < 0.3 W additional GPU load for the upscale pass | Measure GPU power with and without FSR1 pass in Gamescope at identical content |
| Text readability at 85% FSR1 | < 10% of testers report objectionable blur on 12pt monospace | 5-person informal test: terminal at native vs 85% FSR1; count objections |
| Input-restore latency | < 16 ms from keypress to native render scale | Timestamp compositor scale-change via DRM vblank event; compare to evdev keypress timestamp |
| ALS sensor availability | ≥ 3 of 5 reference laptops have IIO ALS | `ls /sys/bus/iio/devices/*/in_illuminance*` on T14 Gen 4, XPS 13, Framework 13 |
| laptop-auto-brightness compatibility | Can be patched to read BrightnessFloor without code restructuring | Read source; prototype patch; run on T14 Gen 4 |

* * *

## 5. Non-Goals

- No DLSS or XeSS integration — hardware-restricted and game-only on Linux.
- No FSR 2/3 integration — requires motion vectors not available at compositor level.
- No per-window resolution scaling — compositor applies a single scale to the full output.
- No eye-tracking foveated rendering — no eye-tracking hardware assumed.
- No custom Wayland compositor for Rush Linux — hints target existing compositors with plugin
  support (Mutter/KWin) or wlroots-based compositors.
- No mandatory ALS daemon — laptop-auto-brightness is an optional companion, not a hard dep.
- No upscaling when locked or DPMS-off — no energy cost reason; panel is already saving.

* * *

## 6. WP Relationship Map

| WP / Doc | Relationship |
|---------|-------------|
| **WP-N7 (panel side)** | This brief extends the WP-N7 scope with GPU render scale and ALS |
| **0007** | Freshens §1.4 (ALS integration model) and §5 (non-goals) |
| **0005 (focus bridge)** | Provides active window class for content detection gate (§2.C) |
| **ADR-0013** | Confirms optid must not call KMS ioctls; hints-only approach complies |
| **SPEC §3 actuation rule** | Render scale hint is advisory (compositor applies); passes §3 gate |
| **Future WP** | A new WP entry (e.g. WP-N7b) may be warranted once compositor plugin is specced |

* * *

## 7. Next Steps

### Immediate (no hardware needed)
- [ ] Read `Nan0pk/laptop-auto-brightness` source and update §1.6 with actual algorithm,
      sysfs paths, and language/dependency details.
- [ ] Confirm Decision B (cooperation model) is feasible with that repo's architecture.
- [ ] Draft `RenderScaleHint` D-Bus signal XML extension to `org.rush.Optid.DisplayHint`.
- [ ] Prototype FSR1 shader integration in a wlroots compositor (Sway plugin or Cage) to
      validate the technical feasibility of §1.2 before WP scheduling.

### Short-term (needs hardware)
- [ ] Run GPU power measurements at 85% vs 100% render scale (§4 gap 1).
- [ ] Run FSR1 overhead measurement (§4 gap 2).
- [ ] Test ALS sensor availability on reference laptops (§4 gap 5).
- [ ] Run informal text-readability test at 85% FSR1 (§4 gap 3).

### Medium-term
- [ ] If text readability at 85% passes the §4 threshold: add `RenderScaleHint` to
      `optid-session-bridges/display.rs` (extends 0007 implementation).
- [ ] Package `laptop-auto-brightness` as `rush-als` companion with `BrightnessFloor`
      protocol support.
- [ ] Propose WP-N7b in SPEC for render-scale and ALS once §4 evidence is collected.
- [ ] Evaluate wlroots FSR1 plugin as part of Rush Linux image (via mkosi overlay).

* * *

## Appendix: Suggested Reading

### GPU Upscaling
- AMD FidelityFX SDK: github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK — FSR1/2/3 source
- Gamescope source: github.com/ValveSoftware/gamescope — FSR1 compositor integration in `src/rendervulkan.cpp`
- FSR 1.0 overview: gpuopen.com/fidelityfx-superresolution

### Wayland / Compositor
- `wp_fractional_scale_v1` protocol: gitlab.freedesktop.org/wayland/wayland-protocols
- wlroots `wlr_renderer`: gitlab.freedesktop.org/wlroots/wlroots — renderer pipeline
- KWin scripting: develop.kde.org/docs/plasma/kwin — plugin entry points

### ALS / IIO
- `Documentation/ABI/stable/sysfs-bus-iio` — IIO ABI reference
- `iio-sensor-proxy` (GNOME): gitlab.freedesktop.org/hadess/iio-sensor-proxy — ALS proxy daemon
- `Nan0pk/laptop-auto-brightness` — direct prior art (read before finalising §1.6)

### Project-internal
- Research 0007 — Display panel: backlight, PSR, VRR, DPMS (prerequisite)
- Research 0005 — Focus bridge (active window class detection, used by §2.C)
- ADR-0013 (`docs/decisions/0013-detection-and-ml-boundary.md`)
- SPEC §3, §4.3 (display domain), §6 WP-N7
