# 0009 — Runtime PM Autosuspend Policy

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0006-hw-allowlist-db-design.md, docs/research/0018-telemetry-runtime-state-observability.md
**Code:** crates/optid/src/actuators/runtime_pm.rs, crates/optid/src/policy.rs

* * *

## 0. Motivation

The Linux kernel's Runtime PM framework provides a unified mechanism for any bus-attached
device to autonomously enter a low-power state after an idle period and wake on demand.
Unlike system suspend (S3/S0ix), runtime PM is transparent to userspace: devices suspend
and resume individually without stopping the CPU or freezing userspace processes.

For optid, runtime PM is both a **DEPTH-ENABLER** (optid tightens `autosuspend_delay_ms`
and enables `control=auto` to save power) and a **telemetry source** (0018 reads
`runtime_status` and `runtime_suspended_time` per device). SPEC §3.2 requires optid to
gate runtime PM deepening on the HWID allowlist (0006 schema) and to journal every write.

Research questions: What is the correct autosuspend delay per device class? How does optid
avoid breaking devices that need wakeup capability? What is the interaction with udev rules
that also set autosuspend? How does optid handle devices that fail to suspend? What is the
network device edge case with Wake-on-LAN and carrier state?

* * *

## 1. Findings

### 1.1 Kernel Runtime PM Architecture

**Q: How does Linux runtime PM work at the sysfs interface level?**

Every device that supports runtime PM exposes these sysfs attributes under
`/sys/bus/<bus>/devices/<dev>/power/` [PROVEN — `Documentation/power/runtime_pm.rst`]:

```
control                  # "auto" | "on"  — enables/disables autosuspend
autosuspend_delay_ms     # idle countdown before ->runtime_suspend() is called
runtime_status           # "active" | "suspended" | "suspending" | "resuming" | "error"
runtime_usage            # reference count; >0 means in use, cannot suspend
runtime_active_time      # cumulative ms spent active since boot
runtime_suspended_time   # cumulative ms spent suspended since boot
wakeup                   # "enabled" | "disabled" — whether device can wake system
```

**Autosuspend flow** [PROVEN — kernel `drivers/base/power/runtime.c`]:

1. Driver calls `pm_runtime_put_autosuspend()` when idle
2. Kernel starts countdown of `autosuspend_delay_ms`
3. After delay, if `runtime_usage == 0`, kernel calls `->runtime_suspend()`
4. On I/O arrival, kernel calls `->runtime_resume()` automatically before dispatching
   the I/O to the device (transparent to the calling application)

The key optid lever is writing `control=auto` and setting `autosuspend_delay_ms` on
target devices. Most drivers default to `control=on` (runtime PM disabled) unless the
driver explicitly calls `pm_runtime_allow()` in its probe handler [PROVEN — many drivers
call `pm_runtime_forbid()` or simply never call `pm_runtime_allow()`; both result in
`control=on`].

### 1.2 Default Delays and optid Targets Per Device Class

**Q: What are the subsystem-specific autosuspend defaults and what should optid target?**

**USB subsystem** [PROVEN — `drivers/usb/core/hub.c`, module param `usbcore.autosuspend`]:

Default: `autosuspend_delay_ms=2000` (2 seconds), applied at device enumeration if
`usbcore.autosuspend` kernel parameter is set (it is not set by default in most
distributions, which means USB autosuspend is disabled by default system-wide).

optid explicitly enables USB autosuspend on a per-device basis for devices in the
allowlist:
- USB HID (mouse, keyboard): enable with 1000 ms delay [HYPOTHESIS — 1000 ms balances
  suspend frequency vs. wake latency; 2000 ms is also fine]
- USB audio: enable with 3000 ms delay; only when no audio stream is active (check
  `/proc/asound/card*/pcm*/sub*/status` for "RUNNING") [HYPOTHESIS]
- USB cameras (UVC): enable with 0 ms delay when camera fd not open [HYPOTHESIS]
- USB Bluetooth adapter: 2000 ms; do not reduce (BT stack manages its own reconnect) [PROVEN]

**PCI subsystem** [PROVEN — `drivers/pci/pci-driver.c`]:

Default: `control=on` (runtime PM disabled) unless driver explicitly enables via
`pm_runtime_allow()`. Drivers that opt in to PCI runtime PM include: `iwlwifi`, `ath11k`,
`r8169`, `nvme-pci`, `xhci-hcd`.

optid enables `control=auto` for opted-in drivers; delay: 2000 ms default, 500 ms during
battery-idle workload class.

**I2C / HID-over-I2C** [PROVEN — `drivers/hid/i2c-hid/i2c-hid-core.c`]:

Touchpads and touchscreens: driver default 2000 ms; optid should not decrease below
500 ms to avoid choppy tracking on first touch after resume [HYPOTHESIS — user-reported
latency on popular Synaptics/ELAN touchpads].

**Bluetooth host controller** [PROVEN — `net/bluetooth/hci_core.c`]:

HCI device gets `autosuspend_delay_ms=2000` after last connection closes. optid should
not alter Bluetooth autosuspend — the BT stack manages idle tracking internally; sysfs
writes to `/power/control` for BT USB adapters may interfere with fast reconnect and
bonded device wake [HYPOTHESIS — conservative based on BT PM architecture].

**Thunderbolt / USB4 root controller** [PROVEN — `drivers/thunderbolt/tb.c`]:

Must NOT autosuspend while any downstream device is connected (child devices are active
via PCIe-over-TB; suspending the root would drop them). optid checks `runtime_usage > 0`
before applying autosuspend to TB root; skips if usage count is positive.

**Summary of optid delay targets** [HYPOTHESIS — to be validated per §4]:

| Device class | Kernel default | Battery-idle target | Hard constraint |
|-------------|---------------|--------------------|----|
| USB HID keyboard/mouse | off | 1000 ms | `wakeup=enabled` required |
| USB audio | off | 3000 ms; skip if stream active | Check ALSA PCM status first |
| USB camera (UVC) | off | 0 ms when no open fd | Check `/proc/*/fd/*` links |
| USB Bluetooth | 2000 ms (if enabled) | 2000 ms | Do not reduce |
| PCIe NVMe | 2000 ms (runtime PM) | 2000 ms | Covered by 0008 APST path |
| PCIe WiFi | 2000 ms | 500 ms | WoL capability check first |
| PCIe Ethernet | off | off if carrier up; 2000 ms if no carrier | WoL check |
| I2C touchpad | 2000 ms | 1000 ms | Not below 500 ms |
| Thunderbolt root | off | off if children present | Skip entirely |

### 1.3 Wakeup Source Protection

**Q: How does optid avoid disabling device wakeup capability when autosuspending?**

`power/wakeup` and `power/control` are independent sysfs attributes [PROVEN — separate
kernel PM attributes, independently settable]. A device can simultaneously have
`control=auto` (autosuspends when idle) AND `wakeup=enabled` (can wake the system from
suspend). This is the correct configuration for USB HID devices: suspend when idle,
wake on keypress.

**optid rule: never write `wakeup=disabled` on input devices** [PROVEN — HID class devices
with `EV_KEY` event capability must retain wakeup enabled for system resume from keyboard].

Device categories where optid must preserve `wakeup=enabled`:
- USB HID keyboard (`/sys/bus/hid/devices/*/power/wakeup`)
- USB HID mouse (human movement should wake the display)
- Power button / lid switch (already managed by systemd-logind; optid does not touch)
- WoL-configured network interfaces (see §1.6)

Device categories where optid may safely set `wakeup=disabled` [HYPOTHESIS]:
- USB cameras (no legitimate wakeup reason)
- USB audio interfaces (no wakeup reason in standard usage)
- USB hubs where all children retain wakeup (hub itself does not need to wake)

When setting `control=auto` on a device, optid explicitly checks `power/wakeup` and logs
a warning if it would be `disabled` on an input-class device — but does not set it.

### 1.4 Conflict with udev-Managed Autosuspend

**Q: How does optid interact with existing udev rules that set autosuspend?**

Many distributions ship udev rules that set autosuspend for known devices:

```udev
# /usr/lib/udev/rules.d/50-usb_power_save.rules (example)
ACTION=="add", SUBSYSTEM=="usb", ATTR{power/autosuspend}="2"
```

**Conflict resolution** [PROVEN — udev runs at device appearance, optid runs after]:

1. udev fires `ACTION=add` → sets initial `autosuspend_delay_ms`
2. optid receives udev `add` notification from its socket → reads current value,
   applies its own policy if the HWID is in the allowlist

optid's write **overrides** the udev-set value. This is intentional: optid has runtime
context (battery vs. AC, workload class, contract floor) that static udev rules cannot
have. The SPEC §3.1 safety gate additionally ensures the override is allowlist-gated.

To avoid unnecessary writes, optid caches the last-applied value per device sysfs path
and skips re-writes when the desired value already matches the current value [HYPOTHESIS
— optimisation to reduce journal churn].

### 1.5 Handling Suspend Failures

**Q: What happens when a device's `->runtime_suspend()` returns an error?**

When runtime suspend fails, the kernel sets the device's RPM state to `RPM_ACTIVE` and
marks a `RUNTIME_ERROR` flag. The `runtime_status` sysfs attribute shows `"error"`
[PROVEN — kernel `drivers/base/power/runtime.c`, `rpm_suspend()` error path].

The device remains active and autosuspend is disabled until the driver calls
`pm_runtime_set_active()` to clear the error (typically after firmware reload or
hardware reset).

**optid error handling**:

1. Detect `runtime_status == "error"` in the 2s telemetry poll loop (from 0018)
2. Log the device sysfs path and the last kernel error via `dmesg` tail scan
3. Increment an error counter in telemetry (`suspend_failures_total` metric)
4. Do NOT retry autosuspend configuration for that device within the current session
   (avoids log spam and driver state confusion) [PROVEN design — conservative]
5. Report via `optctl status` as a `"suspend-error"` device entry

If the error clears (device re-bind after firmware reload or udev `DRIVER_UNBIND`/`BIND`
cycle), optid re-evaluates the device on the next `udev ACTION=bind` event.

### 1.6 Network Devices: WoL and Carrier Check

**Q: How does optid safely autosuspend network interfaces?**

Network device autosuspend is complicated by two requirements:

1. **Wake-on-LAN (WoL)** capability must be preserved if the user configured it
2. **Active carrier** (link up) means the device is in use — autosuspend would silently
   drop network packets

**WoL detection** [PROVEN]:
```bash
ethtool <iface> | grep "Wake-on:"
# "d" = disabled; anything else means WoL is active (g=magic packet, p=phy, etc.)
```
If WoL is enabled, optid sets `wakeup=enabled` on the underlying PCIe device and does
NOT reduce `autosuspend_delay_ms` below the kernel default.

**Carrier detection** [PROVEN]:
```bash
cat /sys/class/net/<iface>/carrier  # 1=link up, 0=link down or no cable
```
If carrier is `1` (link up), optid skips autosuspend configuration for the PCIe device
backing that network interface. Carrier state is re-checked on each 2s telemetry poll;
when carrier drops to `0`, optid re-evaluates whether autosuspend is now appropriate.

**Wireless interfaces** (WiFi): carrier is up when associated to an AP. WiFi runtime PM
interacts with the driver's own power-save implementation (`iw dev wlan0 set power_save on`);
optid sets PCIe-level autosuspend to 500 ms on battery idle but defers Wi-Fi power-save
mode to the driver/NetworkManager [HYPOTHESIS — double-managing Wi-Fi PS causes issues].

### 1.7 Interaction with System Suspend

Runtime PM and system suspend are orthogonal but interact at suspend entry [PROVEN —
`Documentation/power/runtime_pm.rst §9`]:

When system suspend is triggered:
1. PM core calls `->suspend()` on all devices sequentially
2. For devices currently in runtime-suspend, PM core normally calls `->runtime_resume()`
   first to bring them to full power, then calls `->suspend()`
3. Exception: drivers annotated with `DPM_FLAG_SMART_SUSPEND` allow PM core to skip the
   runtime-resume step and go directly from runtime-suspended to system-suspended

`DPM_FLAG_SMART_SUSPEND` has been implemented in many NVMe/USB drivers since kernel 4.15
[PROVEN — `include/linux/pm.h`; nvme-pci, xhci-hcd, iwlwifi set this flag].

The implication for optid: devices already in runtime-suspend when system suspend is
triggered = faster suspend entry (fewer `->runtime_resume()` + `->suspend()` round-trips).
This is a free benefit of runtime PM — optid does not need special handling.

### 1.8 Allowlist Schema for Runtime PM

Following the 0006 schema, runtime PM entries use `domain = "runtime_pm"`:

```toml
[[allowlist]]
domain = "runtime_pm"
hwid = "usb:v046Dp0082"   # Logitech Unifying Receiver
max_state = "auto"         # enable autosuspend
autosuspend_delay_ms = 1000
tested_on = ["ThinkPad X1C Gen 11"]
reason = "USB HID receiver; wakeup preserved; no observed wakeup latency issues"
added_in = "0.1.0"
audit_priority = "low"

[[allowlist]]
domain = "runtime_pm"
hwid = "pci:v8086d2723"   # Intel AX200 WiFi
max_state = "auto"
autosuspend_delay_ms = 500
tested_on = ["Dell XPS 13 9315"]
reason = "PCIe WiFi; L1.2 excluded (see 0008); runtime PM safe at 500ms"
added_in = "0.1.0"
audit_priority = "medium"
```

* * *

## 2. Architecture Decisions

### Decision A: Global Policy vs. Per-Device Control

**Selected: Per-device control** gated on HWID allowlist, workload class, and wakeup
capability check [PROVEN — matches SPEC §3.1 gate; blanket `echo auto > /sys/bus/usb/devices/*/power/control` is too broad and can break non-allowlisted devices].

### Decision B: Delay Values — Static vs. Dynamic

**Selected: Two static delay levels per device class**:
- `normal` (AC, or a workload class busier than `light`): preserve kernel default
- `battery-quiet` (battery + `idle` **or** `light` workload class): apply
  tightened delay from §1.2 table

> **Amended 2026-08-22.** The trigger was originally `battery` + `idle` only.
> That proved unreachable on real hardware: `idle` requires a 1-minute load
> average at or below 0.05, and a laptop with a logged-in desktop session idles
> around 0.3 (measured: 0.31 on the HP Victus 16-r0xxx laptop slot with nothing
> running). The lever was therefore dead on every machine with a user on it —
> which is every machine that has a battery worth saving. `light` is the class
> for "barely doing anything", which is when suspending an idle device is
> appropriate. Backlight dimming deliberately stayed on `idle` alone: an idle
> device suspending is invisible, a panel dimming while someone reads is not.

Dynamic per-context delay calculation is premature optimisation for v0.1; two levels
cover 95 % of practical battery-saving scenarios [HYPOTHESIS — can be revisited in v0.2
based on telemetry data from the field].

### Decision C: Error Handling — Retry vs. Skip

**Selected: No retry within session** for devices with `runtime_status=error`. Re-evaluate
on `ACTION=bind`. This avoids log spam and respects the driver's error state machine
[PROVEN design — conservative, matches libinput/PulseAudio convention of not retrying
failed device operations].

### Decision D: Carrier-Aware Network Skip

**Selected: Skip autosuspend for network devices when carrier is up**. Check at policy
application and re-evaluate on each 2s poll. This is simpler than a netlink listener
and sufficient at 2s granularity [HYPOTHESIS — netlink is more efficient but adds
complexity; revisit if carrier state flip introduces > 4s autosuspend lag].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| USB HID wakeup latency | Wake latency < 10 ms (imperceptible to user) | `evtest` with `perf stat`; measure timestamp from hardware interrupt to evdev event; repeat at autosuspend=1000ms |
| PCIe WiFi autosuspend power saving | ≥ 0.2 W reduction vs. `control=on` | `turbostat --interval 2` with WiFi idle (no traffic), compare `control=auto 500ms` vs. `control=on` |
| WoL magic packet delivery | 0 missed packets while NIC at runtime-suspend | `etherwake` magic packet; confirm system wakes; repeat 20× |
| I2C touchpad first-touch latency | ≤ 50 ms at autosuspend=1000ms | `evtest` timestamp diff: first touch event after ≥ 1s idle vs. active state |
| Suspend failure rate | 0 `runtime_status=error` on 5-laptop reference set after 1h battery-idle | Boot 5 reference laptops; optid battery-idle 1h; count `runtime_status=error` via `optctl status` |
| System suspend speedup | ≥ 10 % faster `systemctl suspend` with runtime-PM pre-suspended devices | `systemd-analyze critical-chain` before/after optid runtime PM enablement |

* * *

## 5. Non-Goals

- optid does not implement per-device "suspend profiles" (TLP's approach of per-HWID
  delay tables shipped as config files) — the 0006 allowlist is the correct mechanism.
- optid does not manage ACPI S3/S0ix system suspend timers — that is systemd-logind's job.
- optid does not set `USB_QUIRK_NO_AUTOSUSPEND` or add USB quirk entries — those require
  kernel patches.
- optid does not control memory DIMM power states.
- optid does not manage CPU C-state selection — that is the cpuidle framework's domain
  (see 0014 for sched_ext influence on idle decisions).
- optid does not manage GPU runtime PM — that is covered by 0011 (dGPU) and the iGPU
  runtime PM (which is always enabled by the i915/amdgpu driver and not user-configurable).

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N3  | Runtime PM autosuspend is the primary DEPTH-ENABLER mechanism for peripheral devices |
| WP-N4  | All autosuspend enablement gated on 0006 HWID allowlist; every write journaled |
| WP-N5  | `runtime_suspended_time` / `runtime_active_time` are key idle-accounting telemetry signals |
| WP-N6  | Peripheral runtime suspend state feeds into the total system power model for WP-N6 budget |

* * *

## 7. Next Steps

**Immediate**
- [x] Implement the runtime-PM actuator (WP-N5 focused core): `Action::RuntimePm` in the
  `actuator.rs` single write funnel, with device-class guards in
  `crates/optid/src/actuators/runtime_pm.rs`. Enumerates `power/control` candidates
  (`sensors::discover_runtime_pm_device_paths`), gates each on the N4 allowlist
  (`domain = "runtime_pm"`), skips network devices whose carrier is up (§1.6), warns but
  never modifies `power/wakeup` (§1.3), journals originals, and reverts on stop
  (`io_util::revert_runtime_pm`). Policy nominates devices only on **battery + idle**
  (Decision B). — **landed**
  - **Deferred (tracked follow-ups, several need §4 hardware):** per-class delay table
    (uses one conservative 2000 ms default for now), USB-audio active-stream / camera-fd
    checks, WoL `ethtool` parsing, `runtime_status=error` dmesg handling, dynamic
    re-disable when returning to AC, and the 0018 telemetry wiring below.
- [ ] Wire `runtime_status`, `runtime_suspended_time`, `runtime_active_time` into 0018
  telemetry sensor at 2s poll cadence (re-adds the 0018 docmap dependency).

**Short-term**
- Populate allowlist with known-safe USB HID vendors (Logitech, Microsoft, Apple Magic,
  ThinkPad TrackPoint).
- Run USB HID wakeup latency experiment (§4 gap #1).
- Implement `optctl status --runtime-pm` command showing per-device suspension stats.

**Medium-term**
- Evaluate netlink-based carrier detection vs. polling for network devices.
- Collect field telemetry to calibrate delay table values in §1.2 (current values are
  HYPOTHESIS; field data will promote them to PROVEN or refine them).
- Investigate `DPM_FLAG_SMART_SUSPEND` adoption rate in target driver set to quantify
  system-suspend acceleration from runtime-PM pre-suspended devices.

* * *

## Appendix: Suggested Reading

- Linux kernel `Documentation/power/runtime_pm.rst` — canonical reference
- `drivers/usb/core/hub.c` — USB autosuspend implementation
- `drivers/pci/pci-driver.c` — PCI runtime PM integration
- `drivers/base/power/runtime.c` — core runtime PM state machine
- TLP source code: `tlp-stat` runtime PM and USB autosuspend reporting
- `pm-graph` tool (Intel): system suspend/resume timeline showing runtime PM interaction
- NetworkManager: `[device] wifi.powersave` config option (complement to PCIe autosuspend)
