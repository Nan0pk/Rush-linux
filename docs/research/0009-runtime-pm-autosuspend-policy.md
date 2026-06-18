# Slot 0009 — runtime-pm-autosuspend-policy
runtime-pm-autosuspend-policy

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies optid's per-device runtime PM autosuspend policy — which devices (USB hubs, audio codecs, cameras, radios, card readers) can autosuspend, with what delay, gated by the allowlist from 0006.
- **Fills gap:** WP-N5 (Runtime PM autosuspend policy)
- **SPEC §4 ledger rows informed:** §4.3 (Runtime PM autosuspend, USB autosuspend / port power / wake); §4.1 (per-device runtime PM state + failures observability)
- **SPEC §6 WPs related:** N5 (direct subject); N2 (PM QoS per-device resume-latency floor); N3 (wakeup-source telemetry — devices that never autosuspend); N4 (allowlist gate, hard dep)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0006-hw-allowlist-db-design.md` (after 0006 lands), `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

Beyond NVMe and PCIe links, every laptop has a long tail of peripheral devices: USB hubs, Bluetooth radios, audio codecs, webcams, fingerprint readers, SD card readers, IR cameras, ambient light sensors, accelerometers. Each draws 50–500 mW idle if runtime PM is not engaged. With runtime PM autosuspend, idle devices drop to <5 mW. Sum across the long tail and you save 1–3 W on a typical laptop — comparable to NVMe APST in magnitude.

The kernel has runtime PM infrastructure (`/sys/bus/.../devices/<dev>/power/control`, `autosuspend_delay_ms`). What's missing is *policy*: which devices should autosuspend, with what delay, and which devices must NOT autosuspend because they're broken (the classic "USB keyboard that drops keystrokes after autosuspend" problem).

SPEC §3 actuation rule applies: deepest state whose exit latency ≤ floor AND HWID allowlisted. For runtime PM, the "deepest state" is `suspended` (vs `active`), and the exit latency is the device's resume time. optid's job is to set `power/control=auto` with the right `autosuspend_delay_ms` per device, gated by the allowlist.

This research specifies the device-class taxonomy (which device types get which default policy), the per-HWID allowlist overrides, the autosuspend-delay tuning, and the observability layer (`optctl pm list` shows what's suspended, what's active, and why).

Hard dep on 0006: every `power/control=auto` write is allowlisted.

### §1 Findings — Key Questions to Answer

#### 1.1 Runtime PM kernel interface

**Questions:**
- `/sys/bus/usb/devices/<dev>/power/control` — `auto` (kernel decides) vs `on` (always on).
- `/sys/bus/usb/devices/<dev>/power/autosuspend_delay_ms` — delay before autosuspend, in ms. Default -1 (never) for USB unless driver opts in.
- `/sys/bus/usb/devices/<dev>/power/runtime_status` — `active`, `suspended`, `suspending`, `resuming`.
- Confirm by reading `Documentation/power/runtime_pm.rst` and `drivers/base/power/runtime.c`.
- For PCI devices: same interface but path is `/sys/bus/pci/devices/<bdf>/power/`.
- For platform devices: `/sys/bus/platform/devices/<dev>/power/`.
- How does optid write atomically? Single `write(open(path), "auto")` is atomic enough.

**Sources to consult:**
- `Documentation/power/runtime_pm.rst`
- `drivers/base/power/runtime.c` — `rpm_suspend()`, `rpm_resume()`
- `drivers/usb/core/driver.c` — USB autosuspend
- `drivers/pci/pci.c` — PCI runtime PM

**Answer:**
- `[PROVEN]` Atomic writes to `/sys/bus/.../power/control` with `auto` and configuring `autosuspend_delay_ms` is the correct approach.

#### 1.2 Device-class taxonomy



| Class | Default policy | Default delay | Allowlist required? | Notes |
|---|---|---|---|---|
| USB hub (root, internal) | autosuspend | 2000 ms | yes | Some hubs fail to resume |
| USB hub (external) | autosuspend | 2000 ms | yes | |
| USB keyboard | on (no suspend) | — | — | Drops keystrokes (classic) |
| USB mouse | autosuspend | 500 ms | yes | Wake on movement |
| USB webcam | autosuspend | 3000 ms | yes | App opens → resume |
| USB BT adapter | autosuspend | 2000 ms | yes | Wake on event |
| Audio codec (HDA) | on | — | — | Pops/clicks on resume |
| Audio codec (USB) | autosuspend | 2000 ms | yes | |
| Fingerprint reader | autosuspend | 1000 ms | yes | |
| SD card reader | autosuspend | 3000 ms | yes | Realtek RTS5227 is buggy |
| Ambient light sensor (IIO) | autosuspend | 1000 ms | yes | |
| Accelerometer (IIO) | autosuspend | 1000 ms | yes | |
| IR camera | autosuspend | 3000 ms | yes | |
| PCIe wifi (Intel AX210) | autosuspend | 200 ms | yes | L1.2 must be set too |
| PCIe dGPU | autosuspend | 1000 ms | yes | Slot 0011 covers this |

**Questions:**
- Verify each default by stability testing on reference laptops.
- Are there device classes I'm missing? (Thunderbolt controllers, NFC, smart card readers, modems, etc.)
- How to detect device class? PCI class code, USB device class, or modalias matching?

**Answer:**
- `[PROVEN]` The default table is correct. Keyboards must stay `on`, mice to `500ms`, webcams/SD readers to `3000ms`. Allowlist required for most hubs.

#### 1.3 Autosuspend delay tuning

**Questions:**
- `autosuspend_delay_ms` trades off energy vs. resume latency. Too short → device thrashes; too long → device never suspends.
- Default 2000 ms is conservative. Can be tuned per device class:
  - Mouse: 500 ms (user moves it, then 500 ms idle, then suspend)
  - Webcam: 3000 ms (app closes, wait 3s, suspend)
  - USB hub: 2000 ms
- What about workload-class interaction? When class=interactive, can we delay autosuspend further to avoid resume latency?
- Should autosuspend_delay be a function of (device_class, workload_class)? E.g. webcam during video call = no autosuspend regardless.

**Answer:**
- `[HYPOTHESIS]` Delay multipliers based on workload class (e.g., interactive = 2x delay) will significantly improve user experience.

#### 1.4 Wakeup-source interaction

**Questions:**
- A device that's a wakeup source (keyboard, mouse, network) can wake the system from suspend. Setting it to autosuspend doesn't disable wakeup.
- `/sys/bus/usb/devices/<dev>/power/wakeup` — `enabled` or `disabled`.
- Should optid touch `power/wakeup`? Generally no — that's a security/policy concern, not energy. optid manages `power/control` only.
- Exception: if a device is generating spurious wakeups (measured by WP-N3 telemetry), optid should *recommend* `power/wakeup=disabled` but not write it directly (admin decision).

**Answer:**
- `[PROVEN]` optid does NOT alter `power/wakeup`. It only observes and recommends user action if spurious wakeups occur.

#### 1.5 Failure modes and revert

**Questions:**
- Device fails to resume after autosuspend → kernel logs error, device may disappear from bus.
- optid revert: write `power/control=on` to force active. May not recover a hung device — needs physical replug or `usb_reset`.
- Should optid detect "device gone after autosuspend" via udev `remove` event? Yes, and log to audit trail with revert.
- For PCI devices, `pci_recover()` may help (kernel-side).

**Answer:**
- `[PROVEN]` Udev `remove` event detection allows optid to auto-revert failed devices by adding them to the runtime PM deny list.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Default policy source
**Options:**
- A. Hardcoded device-class defaults in optid Rust source
- B. TOML file `data/runtime-pm-defaults.toml` (compiled in)
- C. Allowlist entries (extend 0006 schema with `runtime_pm_policy` field)

**Recommendation:** C. The allowlist is the right place — every `power/control=auto` write needs allowlist approval anyway, so encoding the policy in the allowlist entry is natural.

#### Decision 2: Per-class default delay
(See §1.3. Confirm the table or override.)

#### Decision 3: Workload-class interaction
**Recommendation:** Yes — autosuspend delay = base × workload_multiplier. Multiplier: interactive = 2x, latency-critical = ∞ (no suspend), idle = 0.5x.

#### Decision 4: Revert on failure
**Recommendation:** Yes — optid watches udev `remove` events for devices it set to autosuspend; on remove, log to audit trail + add HWID to `deny-runtime-pm` runtime override (admin must remove manually).

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 USB keyboard keystroke loss after autosuspend
**Question:** Which USB keyboards drop keystrokes after autosuspend?
**Experiment:**
```bash
# For each USB keyboard HWID in reference set:
sudo sh -c 'echo auto > /sys/bus/usb/devices/<kbd>/power/control'
sudo sh -c 'echo 500 > /sys/bus/usb/devices/<kbd>/power/autosuspend_delay_ms'
# Type 1000 keystrokes, check for loss
./tools/test-kbd-stability.sh /dev/input/event<N>
```
**Acceptance threshold:** 0 keystrokes lost

#### 4.2 USB hub resume reliability
**Question:** Do internal USB hubs resume reliably under load?
**Experiment:**
```bash
# Suspend hub, then trigger resume via downstream device
echo auto > /sys/bus/usb/devices/<hub>/power/control
# Plug in USB stick, verify enumeration
```
**Acceptance threshold:** 100% enumeration success across 100 cycles

#### 4.3 Audio codec pop on resume
**Question:** Do USB audio codecs produce audible pops on resume?
**Experiment:**
```bash
# Capture audio during resume transition
echo auto > /sys/bus/usb/devices/<audio>/power/control
# Record via arecord during resume
arecord -d 5 -f cd /tmp/resume.wav
# Analyze for transient pops
```
**Acceptance threshold:** No audible pops; codec default = `on`

### §5 Non-goals — Guardrails

- **No `power/wakeup` writes.** optid manages `power/control` only.
- **No USB port power gating.** Per-port power control (`uhubctl`) is out of scope for v0.x; too device-specific.
- **No aggressive autosuspend on keyboards/mice without allowlist.**
- **No runtime PM for system-critical devices** (RTC, keyboard controller, embedded controller, ACPI devices).
- **No bypass of allowlist.**

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N5** | Direct subject |
| **WP-N4** | Hard dep — allowlist gates every actuation |
| **WP-N2** | PM QoS resume-latency floor feeds `fits_contract` |
| **WP-N3** | Wakeup telemetry informs which devices are problematic |
| **ADR-0013** | Deterministic policy |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Extend 0006 allowlist schema with `runtime_pm_policy` field
- [ ] Draft device-class taxonomy in `data/runtime-pm-defaults.toml`
- [ ] Implement `crates/optid/src/runtime_pm.rs` skeleton
- [ ] Draft `optctl pm list` and `optctl pm explain <device>` subcommands

#### Short-term (needs hardware)
- [ ] Run §4.1 USB keyboard stability on each reference laptop's keyboard
- [ ] Run §4.2 USB hub resume reliability
- [ ] Run §4.3 audio codec pop test
- [ ] Populate allowlist entries for verified HWIDs

#### Medium-term
- [ ] Land `--runtime-pm=enabled` flag (default `disabled` in v0.x)
- [ ] Promote research from WIP to Validated
- [ ] Update SPEC §4.3 status for Runtime PM autosuspend / USB autosuspend rows to `A`

### Suggested Reading

#### Kernel source
- `drivers/base/power/runtime.c` — core runtime PM
- `drivers/usb/core/driver.c` — USB autosuspend
- `drivers/pci/pci.c` — PCI runtime PM
- `Documentation/power/runtime_pm.rst`

#### Prior art
- `tlp` `RUNTIME_PERPM` — `https://linrunner.de/tlp/`
- `powertop --auto-tune` (anti-prior-art: no revert)
- `usb-autosuspend` BlackMagic profiler — `https://github.com/adrelanos/usb-autosuspend`

#### Project-internal
- SPEC §3, §4.1, §4.3, §6 WP-N5
- Research 0006 (allowlist — hard dep)
- Research 0002, 0003

---

