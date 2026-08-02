# Render scaling and ambient-light feasibility disposition

Status: research disposition for package R3. This paper does not authorize a
production feature, change runtime defaults, or override the Northstar, accepted
decisions, the completion plan, or the package ledger.

Updated: 2026-08-03

## Question

Can Rush add desktop render scaling or ambient-light brightness control through
a bounded, reversible, privacy-safe interface without taking ownership from the
compositor, desktop power manager, kernel, or user?

## Decision summary

| Capability | Disposition | Boundary |
|---|---|---|
| General desktop render-at-lower-resolution plus upscaling | **Defer** | No stable cross-compositor mechanism was established. |
| Gamescope spatial upscaling | **Manual feasibility harness only** | Nested, per-application, off by default, never an optid actuation path. |
| GNOME/Mutter render scaling | **Defer** | Public display configuration exposes monitor modes and logical scale, not a stable power-saving render/upscale contract. |
| KDE/KWin render scaling | **Defer** | Output configuration and effects do not establish a supported optid-owned internal-resolution contract. |
| wlroots render scaling | **Compositor-specific research only** | Output-management protocols configure outputs; a renderer/upscaler still requires compositor code and ownership. |
| Hardware ambient-light sensors | **Use the desktop session owner** | Observe through IIO/iio-sensor-proxy where available; the session power manager or compositor owns preference and brightness application. |
| Webcam-derived ambient light | **Reject as an automatic Rush service** | Camera activation is privacy-sensitive and the reviewed project is a webcam application, not an IIO sensor daemon. |
| Direct optid backlight writes for ALS | **Reject** | They would compete with user/session brightness ownership and bypass manual override semantics. |

The package result is deliberately conservative: no production package or
feature flag is authorized by this review.

## Ownership rules

1. The compositor owns how a desktop frame is rendered, scaled, filtered, and
   presented.
2. The desktop session power manager owns automatic brightness preference and
   user override behavior.
3. The kernel and sensor stack own hardware discovery and raw sensor delivery.
4. optid may eventually provide advisory context, but an advisory signal must
   not become an implicit write authority.
5. Missing, stale, denied, or disappearing interfaces must return to native
   rendering and user-selected brightness without a privileged recovery write.

## Render-scaling feasibility

### GNOME and Mutter

Mutter's display configuration surface manages monitor modes, layout, and
logical scale. That is useful for user display configuration, but it does not
establish a stable API for rendering a desktop at a lower internal resolution,
applying a spatial upscaler, and preserving a native scanout mode.

A private compositor plugin, downstream patch, or shell extension would not be
a portable optid contract. Rush therefore must not emit a GNOME render-scale
request until Mutter exposes and documents an owner-controlled mechanism with
reversible semantics and compatibility guarantees.

**Disposition:** defer. Native rendering is the fallback.

### KDE Plasma and KWin

KScreen and KWin expose output mode, layout, and scale configuration. KWin also
owns composition effects and brightness integration. Those surfaces do not make
an external daemon the owner of an internal-resolution/upscaling pipeline.

A KWin-specific effect or downstream patch could be studied by the desktop
maintainer, but it would remain compositor code. optid must not treat output
scale configuration as a substitute for a power-saving render-scale mechanism.

**Disposition:** defer. Native rendering is the fallback.

### wlroots compositors

wlroots provides renderer building blocks and output-management protocols, but
the output-management protocol describes output configuration rather than a
standard shader-injection or lower-internal-resolution contract. Each
compositor decides its renderer, scene graph, damage handling, text path, and
presentation behavior.

A prototype is therefore compositor-specific. It cannot become a generic optid
feature merely because wlroots exposes low-level renderer APIs.

**Disposition:** research may occur inside one selected compositor after owner
acceptance; no generic Rush bridge is authorized.

### Gamescope

Gamescope is a nested micro-compositor with explicit input and output dimensions
and spatial upscaling options, including FSR. It is the only reviewed path that
can form a bounded manual feasibility harness without changing the host desktop
compositor.

A representative nested experiment is:

```sh
gamescope -w 1280 -h 720 -W 1920 -H 1080 -F fsr -f -- <application>
```

This command is a test recipe, not a default, package integration, or claimed
measurement. The experiment must run nested in the existing session, be started
by the user for one application, and restore the unchanged host session by
exiting Gamescope. Rush must not launch it automatically, request DRM/KMS
ownership, or reinterpret a successful launch as evidence of power savings.

The harness may be used only to measure:

- GPU and whole-system power at native and reduced render dimensions;
- upscaler overhead;
- text and UI quality at the chosen dimensions;
- input-to-native-restoration behavior if a reversible wrapper is later built;
- application and driver compatibility; and
- clean recovery when Gamescope or the client exits unexpectedly.

**Disposition:** manual, off-by-default feasibility measurement only. No optid
runtime code is authorized.

## Ambient-light feasibility

### Hardware sensor path

Linux IIO defines illuminance channels, and iio-sensor-proxy exposes ambient
light to desktop sessions over D-Bus on supported hardware. This is the correct
starting point because it preserves kernel device ownership and a session-level
consumer.

A production design would still need all of the following before scheduling a
package:

- stable sensor identity and units;
- explicit handling of absent, stale, implausible, or disappearing readings;
- per-user and per-display calibration;
- bounded smoothing and update rate;
- a visible enable/disable control;
- immediate and durable manual override;
- no adjustment while locked, display-off, or session-inactive;
- clean handback when the sensor service or desktop bridge disappears; and
- no direct root backlight write by optid.

The desktop owner should combine sensor preference, user preference, display
capability, and accessibility requirements. optid may later offer an advisory
battery or thermal constraint only through a separately accepted session
contract.

**Disposition:** hardware ALS belongs to the desktop session owner. This review
does not authorize optid to poll IIO or write brightness.

### `Nan0pk/laptop-auto-brightness`

The reviewed repository describes a cross-platform tray application that
estimates ambient brightness from a webcam for computers without an ambient
light sensor. It allows the user to choose a camera and interval and can apply
brightness through platform-specific backends such as sysfs or Windows
`powercfg`.

It is therefore not the previously assumed IIO sensor daemon and is not a safe
background dependency for optid. Packaging it as an automatic system service
would introduce camera activation, image capture, desktop UI, and brightness
ownership into a privileged power-management path.

A user may independently choose such an application, but Rush must not activate
a camera for ambient-light estimation without explicit foreground consent and a
clear indicator. No camera-derived value may silently enter optid policy.

**Disposition:** reject webcam ALS as an automatic Rush or optid feature. Do not
package this repository as the R3 hardware-sensor backend.

## Fail-passive behavior

| Failure or absence | Required result |
|---|---|
| No supported compositor mechanism | Keep native rendering. |
| Gamescope unavailable or fails | Exit the manual experiment; leave the host session unchanged. |
| Upscaler quality or power benefit unproven | Do not schedule production integration. |
| No hardware ALS | Preserve user-selected brightness and report unsupported. |
| Sensor value stale, malformed, or implausible | Stop automatic adjustment and hand control to the user/session. |
| Sensor or D-Bus service disappears | Stop adjustment; do not reuse the last reading indefinitely. |
| User changes brightness | Manual preference wins immediately. |
| Session locks or becomes inactive | Stop camera/sensor-driven adjustment. |
| Camera consent absent | Never open the camera. |
| Desktop brightness owner is unknown | Do not write. |

## Evidence still required

No new physical measurement was performed for this disposition. Before any
render-scaling package can be proposed, the owner and desktop maintainer must
accept an experiment plan and the evidence must show a worthwhile repeatable
benefit without unacceptable quality, latency, compatibility, or recovery cost.

At minimum, a render-scaling evidence bundle must record:

- host compositor, Gamescope version, GPU, driver, kernel, display mode, and
  application;
- exact input/output dimensions and upscaler mode;
- idle and active power baselines with repeated samples;
- frame-time and input-latency impact;
- text/UI quality observations using predefined content;
- crash and abnormal-exit restoration; and
- unsupported combinations and failures, not only successful runs.

Before a hardware-ALS package can be proposed, evidence must cover real sensor
availability, unit correctness, calibration, manual override, session locking,
service loss, multi-display behavior, and accessibility review. Synthetic lux
values cannot substitute for hardware evidence when making a hardware support
claim.

## Package-state consequence

This paper supplies the implement/defer/reject disposition requested by R3. It
makes no runtime change and deliberately leaves R3 `ready_parallel` in the
ledger until a maintainer and desktop owner accept the disposition as the
package outcome. Acceptance may close R3 as a research decision without
creating a render-scaling or ALS implementation package.

R3 must not be marked `completed` by the author of this paper. No dependency is
unlocked by this document.

## Sources

- Gamescope repository and nested/upscaling usage:
  <https://github.com/ValveSoftware/gamescope>
- Mutter project and display server/compositor ownership:
  <https://mutter.gnome.org/>
- wlroots output-management protocol:
  <https://wayland.app/protocols/wlr-output-management-unstable-v1>
- Linux IIO light-sensor ABI:
  <https://docs.kernel.org/admin-guide/abi-testing-files.html#abi-file-testing-sysfs-bus-iio>
- iio-sensor-proxy D-Bus interface:
  <https://hadess.pages.freedesktop.org/iio-sensor-proxy/gdbus-net.hadess.SensorProxy.html>
- KDE PowerDevil automatic-brightness tracking:
  <https://invent.kde.org/plasma/powerdevil/-/issues/9>
- Reviewed webcam brightness project:
  <https://github.com/Nan0pk/laptop-auto-brightness>
