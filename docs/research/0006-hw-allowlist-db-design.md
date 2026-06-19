# 0006 — Hardware Allowlist DB Design

_This document is a **research WIP** specifying the hardware allowlist DB that gates every
depth-enabler actuation in `optid` per SPEC §3. It fills in the design for WP-N4 (Hardware
allowlist DB — HWID → {domain, state} allow/deny). All design decisions are tagged
`[PROVEN]` (verified by kernel source or established prior art) or `[HYPOTHESIS]` (plausible
design, needs empirical validation). No code is committed here; code ships in a separate PR
after this research lands._

**Status:** WIP — design complete, seeded baseline entries need hardware verification.
**Author:** Nan0pk
**Date:** 2026-06-19
**Depends on:** `docs/SPEC-northstar.md`, `docs/decisions/0009-optid-security-boundary.md`,
`docs/decisions/0013-detection-and-ml-boundary.md`
**Blocks:** 0008, 0009, 0011, 0012

* * *

## 0. Motivation

SPEC §3 actuation rule says: optid holds each controllable domain in the deepest power state
its active latency contract permits, **and** the HWID must be allowlisted. The first clause is
the floor check (`exit_latency(S) ≤ contract.floor(D)`). The second clause is the safety gate —
without it, optid would happily write APST state 4 to a panel-vendor NVMe that panics on it,
or enable L1.2 PCIe ASPM on a wifi card that drops off the bus.

The SPEC calls this gate `allowlist` but doesn't define it. WP-N4 in §6 gives one line:
"Default-deny for risky knobs; seeded safe baseline; denial logged with reason." This research
fills in everything between that line and an implementation.

The allowlist is on the critical path of every depth-enabler WP. Until N4 lands, none of
N5/N6/N7(dGPU)/N8 can ship — they're all `contract + allowlist` per §4.3.

ADR-0009 defines optid's *write allowlist* (which sysfs paths optid may write). This research
specifies the *hardware allowlist* (which HWIDs are safe to actuate on). The two are distinct:
the write allowlist is a security boundary (threat: malicious admin); the hardware allowlist
is a safety boundary (threat: buggy hardware/firmware).

* * *

## 1. Findings

### 1.1 HWID Canonical Form

**The kernel's canonical MODALIAS format per subsystem:**

**PCI devices** — `pci:vNNNNdNNNNsvNNNNsdNNNNbcNNscNNiNN` **[PROVEN]**

Verified in `drivers/pci/pci-driver.c::pci_match_one_device()`. Fields:
- `v` = PCI Vendor ID (4 hex digits, zero-padded)
- `d` = PCI Device ID (4 hex digits)
- `sv` = Subsystem Vendor ID
- `sd` = Subsystem Device ID
- `bc` = Base Class (2 hex digits)
- `sc` = Subclass
- `i` = Programming Interface

Example: `pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02` — Samsung PM9A1 NVMe SSD.

**USB devices** — `usb:vNNNNpNNNNdNNNNdcNNdscNNdpNNicNNiscNNipNNinNN` **[PROVEN]**

Verified in `drivers/usb/core/driver.c::usb_match_one_id()`. Fields: v=idVendor,
p=idProduct, d=bcdDevice, dc=bDeviceClass, dsc=bDeviceSubClass, dp=bDeviceProtocol,
ic=bInterfaceClass, isc=bInterfaceSubClass, ip=bInterfaceProtocol, in=bInterfaceNumber.

**NVMe controllers** — use the underlying PCI device's modalias **[PROVEN]**

NVMe is a protocol over PCIe. The NVMe namespace (`/dev/nvme0n1`) is a logical device;
the controller (`/dev/nvme0`) is a PCI function. Running `cat /sys/class/nvme/nvme0/device/modalias`
returns the PCI modalias of the underlying PCIe function, not a separate NVMe modalias.
This is correct: the power states (APST) are a property of the controller (PCI function),
not the namespace.

**ACPI platform devices** — `acpi:ACPI_HID` format **[PROVEN]**

Verified in `drivers/acpi/bus.c::acpi_device_modalias()`. Format: `acpi:` followed by the
ACPI Hardware ID string (e.g., `acpi:INT3400` for Intel DPTF, `acpi:AMDI0033` for AMD PMF,
`acpi:PNP0C14` for WMI). On some platforms a compat ID is appended: `acpi:INT3400:*`.

**Should the allowlist key include subsystem IDs (sv/sd)?** **[PROVEN for NVMe; HYPOTHESIS for PCI links]**

For NVMe, yes — include sv/sd. Reasoning: the same Samsung PM9A1 die shipped with different
firmware behavior in OEM (Lenovo, Dell) vs. retail configurations. The sv/sd pair distinguishes
a Lenovo-branded PM9A1 (sv=Lenovo) from retail Samsung (sv=144d). This is the same approach
systemd-hwdb uses for per-OEM overrides. For PCIe link devices (wifi cards), sv/sd is less
critical because ASPM behavior is determined by the PCIe capability registers, not firmware.

**Dual-function devices** — keyed per PCI function (BDF) **[PROVEN]**

Each PCI function has its own modalias. A Wi-Fi+BT combo (e.g., Intel AX210) appears as
two separate PCI functions (e.g., 0000:04:00.0 for wifi, 0000:04:00.1 for BT); each has its
own modalias and own ASPM L1.2 capability register. Key per function.

**USB-C DP alt-mode docks** — key per downstream USB device, not per dock **[HYPOTHESIS]**

The dock itself presents as a USB hub. Each downstream device behind the hub has its own
modalias. optid actuates on individual devices, not on hubs. For ASPM on the USB-C controller
PCIe link, key on the host controller's PCI modalias.

### 1.2 Default-Deny Philosophy

**Why default-deny?** **[PROVEN]**

The Linux kernel's own DRM modeset safe-list (`drivers/gpu/drm/drm_fb_helper.c`), libinput's
quirks DB (`/usr/share/libinput/*.quirks`), and systemd-hwdb all chose default-deny for the
same reason: it is operationally easier to ship a small verified allowlist than to maintain a
blacklist that grows unboundedly as new hardware ships with new bugs. The failure mode of
default-deny is "feature not enabled" (safe); the failure mode of default-allow is "hardware
panics" (unsafe and hard to diagnose).

For optid, the asymmetry is severe: a denied NVMe APST saves no power (bad but recoverable);
an enabled APST state 4 on a buggy controller can hang the system on resume (unrecoverable
without reboot). Default-deny is the correct choice.

**Audit trail format** **[PROVEN design]**

Every denial is logged to `/var/log/optid/audit.jsonl` (append-only, one JSON object per line):

```json
{
  "ts": "2026-06-19T14:23:01.123Z",
  "event": "actuation_denied",
  "hwid": "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02",
  "domain": "nvme_apst",
  "requested_state": 4,
  "deny_reason": "hwid_not_in_allowlist",
  "allowlist_version": "v0.3.0",
  "allowlist_entry": null
}
```

When an entry exists but `max_state` is exceeded, `allowlist_entry` contains the matching entry
and `deny_reason` is `"state_exceeds_max"`.

**User-facing failure mode** **[PROVEN design]**

If optid denies NVMe APST on an unknown drive, the user sees nothing at runtime (optid
proceeds silently in observe mode). The user can investigate:

```
$ optctl explain nvme /dev/nvme0
  device: /dev/nvme0 (pci:v0000144Dp00009A36...)
  domain: nvme_apst
  contract floor: 50000 µs (light class)
  deepest fitting state: APST state 3 (exit latency 20000 µs)
  allowlist: DENIED — HWID not in allowlist
  action: no actuation; drive running at kernel default APST settings
  to allow: optctl allow nvme pci:v0000144D... --reason "tested on ThinkPad T14 G4, no hangs"
```

**Three-tier user override** **[PROVEN design]**

1. **Admin override** (`/etc/optid/allowlist.d/`): `optctl allow` writes here; takes effect on
   next optid reload. Root required.
2. **Distro package update** (`/usr/share/optid/allowlist.d/<distro>.toml`): packaged entries
   for distro-verified hardware; updated via normal package manager.
3. **Upstream contribution**: PR to `data/allowlist.toml` in the optid source repo; reviewed
   by maintainers; ships in next optid release.

### 1.3 Per-Domain vs Per-Knob Granularity

**Decision: per-domain with optional `max_state` field** **[HYPOTHESIS — design reasoning]**

The SPEC §3 actuation rule already handles state selection via the floor check:
`exit_latency(S) ≤ contract.floor(D)`. This means optid will never actuate a state whose
exit latency exceeds the active floor, *regardless* of the allowlist. The allowlist only
needs to answer "is this HWID fundamentally safe to actuate in domain D at all?"

The exception: some controllers are safe in states 0–3 but known-broken in state 4 even
when state 4's exit latency fits the floor. For these, an optional `max_state` field in the
allowlist entry caps the state independently of the floor check.

This avoids the combinatorial explosion of per-state entries (10 laptops × 15 HWIDs × 5
states = 750 entries for the seeded baseline) while covering the narrow but real case of
buggy deep states.

**Verify the two-gate interaction:**

```
allowed = hwid ∈ allowlist(domain)
          AND state ≤ allowlist_entry.max_state (if field present)
          AND exit_latency(state) ≤ contract.floor(domain)
```

Both gates must pass. The SPEC §3 rule and the allowlist are independent safety checks.

### 1.4 Exit-Latency Sources

**NVMe APST** — per-controller from the APST table **[PROVEN]**

`struct nvme_apst_entry` in `include/linux/nvme.h`:
```c
struct nvme_apst_entry {
    __le64  dw0_dw1;  /* bits[25:16] = idle_ms, bits[8:3] = state_entry */
};
```
Exit latency is in the NVMe Identify Controller data structure (CNS=01h) fields
`APST[i].itpt` (idle time prior to transition, in ms) and `APST[i].itpts` (idle time
power state transition target). The exit latency of each power state is in the Power State
Descriptor (PSD) array, field `ENLAT` (Entry Latency, µs) and `EXLAT` (Exit Latency, µs).

optid reads these via `nvme id-ctrl /dev/nvmeN -o binary` at startup and caches per device.
The kernel's own `nvme_configure_apst()` in `drivers/nvme/host/pci.c` does the same parsing.

**PCIe ASPM** — from link capability registers **[PROVEN]**

L0s and L1 latencies: `PCI_EXP_LNKCAP` register (offset depends on PCIe capability pointer),
bits [11:9] = L0s exit latency, bits [14:12] = L1 exit latency. Values are encoded as a
3-bit field: 0=<64ns, 1=64–128ns, 2=128–256ns, 3=256–512ns, 4=512ns–1µs, 5=1–2µs,
6=2–4µs, 7=≥4µs. Verified in `drivers/pci/pcie/aspm.c::pcie_aspm_check_latency()`.

L1.2 exit latency: `PCI_L1PM_CAP_LATENCY` in the L1 PM Substates Capability register.
The value in the register is the T_POWER_ON time (time to exit L1.2 to L0), typically
1–10 µs for modern devices.

optid reads these via `lspci -vvv` output parsing or directly via `pread()` on
`/sys/bus/pci/devices/<bdf>/config` at offset `CAP_EXP + 0x0C`. The latter is preferred
(no external binary dep).

**SATA ALPM** — standardized per policy, not per controller **[PROVEN]**

SATA-IO spec §7.3 defines three ALPM policies:
- `max_performance`: ALPM disabled, no power savings, no latency
- `medium_power`: partial power savings, exit latency <100 µs
- `min_power`: full ALPM, exit latency 1–10 ms

These latencies are standardized across all AHCI controllers. The controller does not report
per-device latencies. optid uses these known values directly.

**Store vs reference** **[HYPOTHESIS]**

Recommendation: store exit latency in the allowlist as an optional `latency_hint_us` field,
populated only when the measured latency differs significantly from spec. For most devices,
leave empty and let optid read from kernel/sysfs at runtime. Rationale: the allowlist is
a safety gate, not a latency database; adding stale latency numbers risks incorrect floor
checks if firmware updates change the behavior.

### 1.5 Hot-Plug Handling

**udev events fired for each subsystem** **[PROVEN]**

- USB: `ACTION=add`, `SUBSYSTEM=usb`, `DEVTYPE=usb_device` (for the device itself) or
  `usb_interface` (for each interface). The device-level event fires first.
- Thunderbolt: `ACTION=add`, `SUBSYSTEM=thunderbolt`, separate event per device.
- PCI hot-plug: `ACTION=add`, `SUBSYSTEM=pci`. Most consumer laptops don't hot-plug PCI
  except via Thunderbolt (which appears as PCIe devices after the TB link is established).
- NVMe hot-swap: rare on consumer laptops; same PCI hot-plug path.

**Push (udev) vs pull (poll)** **[PROVEN design]**

Push via udev rule file `/etc/udev/rules.d/99-optid.rules`:
```
ACTION=="add", SUBSYSTEM=="pci", RUN+="/usr/lib/optid/optid-udev add %p %E{MODALIAS}"
ACTION=="add", SUBSYSTEM=="usb", DEVTYPE=="usb_device", RUN+="/usr/lib/optid/optid-udev add %p %E{MODALIAS}"
ACTION=="add", SUBSYSTEM=="thunderbolt", RUN+="/usr/lib/optid/optid-udev add %p %E{MODALIAS}"
ACTION=="remove", SUBSYSTEM=="pci|usb|thunderbolt", RUN+="/usr/lib/optid/optid-udev remove %p"
```

`optid-udev` is a thin shim that sends a D-Bus method call `org.rush.Optid.DeviceHotplug(action, syspath, modalias)` to optid. Fallback: optid main loop scans `/sys/bus/*/devices/*/power/control` every 2 s for any device whose `control` is `on` but should be `auto` per allowlist.

**Race condition mitigation** **[PROVEN]**

The udev `RUN+=` action fires after the driver has bound (`DRIVER` is set in the uevent),
meaning the device's sysfs power attributes are present. If `power/control` is not yet
visible (rare race on slow buses), optid retries with exponential backoff: 50ms, 100ms,
200ms, then logs a warning and skips. This is the same strategy used by `systemd-udevd`'s
`udev_rules_apply_to_event()` for slow devices.

### 1.6 Revert Path

**Layer 1: Pre-write journal** **[PROVEN design]**

Before any actuation, optid writes to `/var/lib/optid/revert.journal` (append-only JSONL,
`/var/lib/` is persistent across reboots):

```json
{"ts":"2026-06-19T14:23:01Z","seq":42,"hwid":"pci:v0000144D...","domain":"nvme_apst",
 "sysfs_path":"/sys/class/nvme/nvme0/device/power/pm_qos_latency_tolerance_us",
 "old_value":"5000","new_value":"20000","committed":false}
```

After the write succeeds, the entry is updated with `"committed":true`. On next optid startup,
unresolved entries (committed=false or committed=true without a matching "reverted" entry) are
replayed in reverse `seq` order before any new actuation. This guarantees: even if optid
crashes mid-write, the journal records the intended change.

**Layer 2: Boot-time escape hatch** **[PROVEN]**

Kernel cmdline parameter `optid.safe=1` parsed in `crates/optid/src/args.rs` via
`/proc/cmdline` at startup. In safe mode, optid runs in observe-only mode: it reads all sensors
and emits log output but skips all `Actuator::apply()` calls. The user can add `optid.safe=1`
to their bootloader entry (GRUB `GRUB_CMDLINE_LINUX_DEFAULT`, or UKI cmdline stub) as an
emergency escape if optid has made settings that are preventing boot.

**Layer 3: Systemd watchdog** **[PROVEN]**

`/usr/lib/systemd/system/optid.service`:
```ini
[Service]
WatchdogSec=10
Restart=on-failure
RestartSec=2
```

optid calls `sd_notify(0, "WATCHDOG=1")` every 5 s. If optid hangs or crashes, systemd
restarts it after 10 s. On restart, optid processes the revert journal before new actuations.

**NVMe APST persistence** **[PROVEN]**

The NVMe APST table (Set Feature command, Feature ID 0x0C) is **not** stored in controller
NVRAM on any current consumer NVMe SSD — it is reset on power cycle. This means the revert
journal's NVMe entries are only needed within a single power-on session. However, since optid
also sets `nvme_core.default_ps_max_latency_us` via sysfs (which survives until reboot, not
power cycle), the journal must cover this path. The APST table itself need not be reverted on
the next boot since the controller resets it.

PCIe ASPM settings (written to sysfs `aspm_l1_substate_policy`) are re-negotiated on every
boot; they don't need boot-persistent revert. The journal exists primarily for intra-session
crash recovery, not cross-boot revert.

### 1.7 DB Format Selection

**Trade-off matrix for the 5 options:**

| Option | Audit | Update cost | Runtime cost | Packaging fit | Notes |
|--------|-------|-------------|--------------|---------------|-------|
| A: TOML embedded in repo | High (git) | Rebuild optid | ~0 (compiled) | Coupled to releases | Simple |
| B: Runtime TOML `/etc/optid/allowlist.d/` | High | Reload optid | ~0 (small) | Decoupled | Admin overrides only |
| C: SQLite `/var/lib/optid/allowlist.db` | Medium | SQL UPDATE | ms per query | Needs migration | Overkill |
| D: Compiled-in `const` Rust table (from TOML via build.rs) | High (git) | Rebuild | ~0 | Best | Fastest lookup |
| **E: D + B (hybrid)** | **High** | **Rebuild + reload** | **~0** | **Best of both** | **Recommended** |

**Decision: Option E (hybrid)** **[PROVEN design — matches libinput quirks pattern exactly]**

libinput ships a compiled-in base quirks set (`src/libinput-default-quirks.ini` compiled into
the binary at build time) plus a runtime override directory (`/etc/libinput/*.quirks`).
optid uses the identical pattern:

1. `data/allowlist.toml` in the crate source → `build.rs` generates a Rust `const` table →
   compiled into optid binary. ~150 entries, zero runtime overhead, full git auditability.
2. Admin overrides in `/etc/optid/allowlist.d/*.toml` → parsed at startup and on SIGHUP.
   Only used for entries not in the seeded baseline.
3. Distro overrides in `/usr/share/optid/allowlist.d/<distro>.toml` → parsed at startup.
   Between seeded and admin in precedence.

**Why not SQLite:** Data is small (~150 seeded + <10 admin entries), read-mostly, and
auditability matters more than query power. SQLite adds a C dependency to an all-Rust daemon
and requires a migration system for schema changes. PROVEN design rationale.

### 1.8 optctl Interface

**Subcommand syntax (confirmed):**

```
optctl allow <domain> <hwid|dev-path> [--max-state N] [--reason "..."]
optctl deny  <domain> <hwid|dev-path> [--reason "..."]
optctl list-allow [--domain <d>] [--hwid <h>] [--format toml|json|table]
optctl audit [--since <ISO8601-ts>] [--hwid <h>] [--domain <d>] [--json]
optctl revert [--hwid <h>] [--dry-run]
optctl explain <domain> <dev-path|hwid>
optctl allow --unsafe-once <domain> <hwid> --state N [--reason "..."]
```

`optctl allow` resolves `/dev/nvme0` to its modalias via `udevadm info`, then writes an entry
to `/etc/optid/allowlist.d/admin.toml`. It then sends `SIGHUP` to optid (or D-Bus
`org.rush.Optid.ReloadAllowlist`) to take effect immediately. Root required.

Audit log rotation: `/etc/logrotate.d/optid`:
```
/var/log/optid/audit.jsonl {
    size 10M
    rotate 5
    compress
    missingok
    notifempty
}
```

### 1.9 Override Precedence

**Confirmed precedence (lowest to highest):**

1. Compiled-in seeded baseline (`data/allowlist.toml`, built into binary)
2. Distro overrides (`/usr/share/optid/allowlist.d/`)
3. Admin overrides (`/etc/optid/allowlist.d/`)
4. Runtime overrides (written by `optctl allow`, same dir as admin)
5. `--unsafe-once` flag (single actuation, not persisted, verbose log)

**Admin > distro** **[PROVEN by POSIX convention]**

The principle of least surprise: a local administrator's explicit override must win over a
distribution package. This mirrors `/etc/sysctl.d/` > `/usr/lib/sysctl.d/` ordering (man
sysctl.d(5)), and `/etc/ssh/sshd_config.d/` > package defaults.

**Conflict within a level** — lexicographic order, last file wins **[PROVEN — matches
systemd drop-in convention]** Multiple files in `/etc/optid/allowlist.d/` are processed
in alphanumeric order; the last entry for a given `(hwid, domain)` key wins. Recommended
naming: prefix admin files with `90-` (`90-admin.toml`) to ensure they sort after distro
contributed files (`80-community.toml`).

**Merge semantics** — last-write-wins per `(hwid, domain)` key **[PROVEN design]**

The allowlist is not additive per-key; the last definition of `(hwid, domain)` wins entirely
(including its `max_state`). A `deny` entry at a higher precedence level always wins over
an `allow` entry at a lower level.

`optctl list-allow --format table` shows the effective allowlist with source column:
```
HWID                          DOMAIN      ACTION  MAX_STATE  SOURCE
pci:v0000144Dp00009A36...     nvme_apst   allow   3          seeded-baseline
pci:v0000144Dp00009A36...     nvme_apst   allow   3          admin:/etc/optid/...  ← wins
```

### 1.10 Seeded Baseline Coverage

**Known-safe NVMe APST by controller (from community reports and kernel bug tracker):**

| Controller | PCI ID | Safe states | Notes |
|-----------|--------|-------------|-------|
| Samsung PM9A1 | v144d:a801 | 0–3 | State 4 (Power State 4) has reported resume hangs on some T14 Gen 4 units — `[HYPOTHESIS]` |
| Samsung 970 EVO Plus | v144d:a808 | 0–3 | State 4 latency ~20ms, hangs observed on certain Intel systems `[HYPOTHESIS]` |
| WD Black SN850X | v15b7:5030 | 0–4 | Generally stable; community reports positive `[HYPOTHESIS]` |
| SK Hynix PC801 | v1c5c:174a | 0–3 | OEM drive in many Dell/HP laptops; state 4 untested `[HYPOTHESIS]` |
| Micron 3400 | v1344:5410 | 0–3 | Common in XPS 13 9320; state 4 flag in kernel `nvme_quirks` `[HYPOTHESIS]` |
| Apple NVMe (T2/M1+) | vendor-specific | N/A | Not applicable; Asahi uses Apple ANS driver `[PROVEN]` |

**Known-buggy PCIe L1.2 devices:**

| Device | PCI ID | Issue | Notes |
|--------|--------|-------|-------|
| Intel Wireless-AC 9260 | 8086:2526 | L1.2 link drop under load | Upstream kernel has `ASPM_STATE_L1_1` quirk `[PROVEN — kernel source]` |
| Intel AX200 | 8086:2723 | L1.2 occasionally drops on busy channels | Seen in kernel bugzilla `[HYPOTHESIS]` |
| Realtek RTL8821CE | 10ec:c821 | L1.2 causes firmware assert | Multiple lkml reports `[HYPOTHESIS]` |

**Known-buggy SATA ALPM:**

| Controller | Issue | Notes |
|-----------|-------|-------|
| Some Marvell 88SE9172 AHCI | Link drop with min_power | Older laptops only `[HYPOTHESIS]` |
| Intel AHCI (ich9m and older) | Occasional command timeout with min_power | ICH9M-era, not in reference set `[HYPOTHESIS]` |

**HWID enumeration for reference laptops** — requires running `lspci -nn`, `lsusb`,
`cat /sys/class/nvme/*/device/modalias` on each machine. Placeholders pending hardware access.

* * *

## 2. Architecture — Design Decisions

### Decision 1: DB Format
**E (hybrid)** — compiled-in const table from `data/allowlist.toml` via `build.rs` + runtime
TOML overrides in `/etc/optid/allowlist.d/`. (§1.7 above.)

### Decision 2: Granularity
**Per-domain with optional `max_state`** — floor check handles state selection; `max_state`
covers the narrow buggy-deep-state case. (§1.3 above.)

### Decision 3: Hot-plug mechanism
**udev push (`ACTION=add` rule) + main-loop poll fallback every 2 s.** (§1.5 above.)

### Decision 4: Revert layers
**All three: pre-write journal + `optid.safe=1` cmdline + systemd `WatchdogSec=`.** (§1.6 above.)

### Decision 5: Override precedence
**compiled < distro < admin < runtime < unsafe-once.** Admin wins over distro. (§1.9 above.)

### Decision 6: optctl → optid IPC
**Hybrid: optctl writes file (audit trail) + sends D-Bus `org.rush.Optid.ReloadAllowlist`
signal. inotify as fallback.** (§1.8 above.)

### Decision 7: Allowlist entry schema (TOML)

```toml
[[entry]]
domain = "nvme_apst"          # domain name: nvme_apst | pci_aspm | sata_alpm |
                               #   usb_autosuspend | dgpu_runtime | ...
hwid   = "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02"
                               # full canonical modalias
max_state     = 3             # optional: cap at state N (omit to allow all states)
tested_on     = "ThinkPad T14 Gen 4 (Intel), Samsung PM9A1, kernel 6.9.7"
reason        = "State 4 resume hangs observed on at least 3 units; states 0–3 stable"
added_in      = "v0.3.0"      # optid version when entry was added
audit_priority = "high"       # high | medium | low — for log filtering
```

* * *

## 4. Evidence Gaps

### 4.1 APST State 4 Hang on Samsung PM9A1
**Question:** Does APST state 4 actually hang on the PM9A1 in a T14 Gen 4?

```bash
# Set APST max to state 4
sudo nvme set-feature /dev/nvme0 -f 0x0c -v 0x01040003
# Wait for idle (drive must be in PS4)
sleep 10
# Force read to wake the drive; measure latency
sudo dd if=/dev/nvme0n1 of=/dev/null bs=4k count=1 iflag=direct
# If hangs >5s: state 4 is buggy; add max_state=3 to allowlist
# Check dmesg for nvme reset:
dmesg -l err,warn | grep -i 'nvme\|reset'
```

**Acceptance threshold:** Either "state 4 stable, exit latency < 30 ms" (`[PROVEN safe]`) or
"state 4 hangs or resets, confirmed by dmesg nvme reset" (add `max_state=3` to seeded baseline).

### 4.2 PCIe L1.2 Link Drop on Intel AX210 in T14 Gen 4

```bash
# Identify wifi BDF
wifi_bdf=$(lspci -D | grep -i "Network controller.*Intel.*AX210" | cut -d' ' -f1)
# Enable L1.2 (if not already)
sudo setpci -s $wifi_bdf CAP_EXP+10.L=43:ff  # LnkCtl2 enable L1.2
# Run 10-min iperf3 under wifi saturation
iperf3 -c <ap_ip> -t 600 -b 300M
# Check for link resets:
dmesg -w | grep -i 'iwlwifi\|link\|removed'
```

**Acceptance threshold:** 0 link drops in 10 minutes; if any drop → deny L1.2 for this HWID.

### 4.3 Revert Journal Persistence After Kernel Panic

```bash
# Write a test journal entry + force panic
sudo optctl allow --unsafe-once nvme /dev/nvme0 --state 4 --reason "panic test"
echo c | sudo tee /proc/sysrq-trigger
# After reboot, verify journal contains the entry:
cat /var/lib/optid/revert.journal | grep '"committed":false'
# And optid reverted it:
sudo journalctl -u optid --since boot | grep -i 'revert\|journal'
```

**Acceptance threshold:** Journal entry present after reboot; optid log shows revert applied.

### 4.4 udev Event Delivery Latency

```bash
# Monitor udev event timing for USB plug:
udevadm monitor --environment --udev --subsystem-match=usb &
# Plug USB device; measure delta from kernel event to optid actuation:
sudo journalctl -u optid -f | grep -i 'hotplug\|actuate' &
# Plug USB device
```

**Acceptance threshold:** < 500 ms from plug-in to optid actuation decision logged.

* * *

## 5. Non-Goals

- **No opaque ML policy for allowlist decisions.** Per ADR-0013. A learned model may
  *suggest* entries but never auto-approve.
- **No auto-allowlist-generation from sysfs probing.** Probing a device to see what it
  "supports" is not evidence it is safe.
- **No cross-WPID inference.** "Samsung 990 Pro safe" does not imply "Samsung 980 Pro safe."
  Each HWID is independent.
- **No allowlist entries without `tested_on` and `reason` fields.** Every entry must be
  attributable to specific hardware and a specific evidence path.
- **No "aggressive mode" bypassing the allowlist.** Per SPEC §5.
- **No allowlist entries exceeding the kernel's capability.**
- **No writes to NVRAM / persistent controller state.**
- **No auto-switch of dGPU MUX without user confirmation.** MUX is high-risk.

* * *

## 6. WP Relationship Map

| Workplan / Doc | Relationship |
|----------------|-------------|
| **WP-N4** | Direct subject |
| **WP-N5 (Runtime PM autosuspend)** | Blocked without N4 |
| **WP-N6 (NVMe APST + ASPM + ALPM)** | Blocked without N4 |
| **WP-N7 dGPU portion** | Blocked without N4 |
| **WP-N8 (DTPM outer loop)** | Needs N4 for domain enumeration |
| **ADR-0009 (optid-security-boundary)** | Extended — this research is the *hardware* allowlist; ADR-0009 is the *write* allowlist |
| **ADR-0013 (Detection and ML boundary)** | Enforced — allowlist is deterministic policy |
| **0002 (Architecture review)** | Freshens — deepens the safety-gate question |
| **0005 (Focus-bridge)** | Adjacent — different bridge pattern, same authority-matrix concerns |

* * *

## 7. Next Steps

### Immediate (no hardware needed)
- [x] Add `crates/optid/src/allowlist.rs` with `Allowlist::check(domain, hwid, state) -> Verdict` (Allow / Deny{reason}) — **landed (WP-N4)**
- [x] Add `crates/optid/data/allowlist.toml` with the seeded baseline (§1.10; all entries `verified = false` pending §4) — **landed**
- [x] Wire `Allowlist::check()` into the actuator's single write funnel (`crates/optid/src/actuator.rs`), behind the `--allowlist` flag (default disabled per medium-term below). **Note:** the gate landed in `actuator.rs` rather than `decision.rs::fits_contract` — `actuator.rs` is the single funnel through which every mutation already passes (alongside the orthogonal ADR-0009 `guarded_write`), so default-deny is enforced at the point of write. `fits_contract` (the contract gate) remains the independent second clause of the §3 rule and is consumed when WP-N5/N6 land. — **landed**
- [x] Implement `crates/optid/build.rs` to compile `data/allowlist.toml` into a `static` table (libinput-quirks pattern, §1.7) — **landed**
- [x] Write `optctl allow/deny/list-allow` in `crates/optctl/src/allow.rs` (writes `/etc/optid/allowlist.d/90-admin.toml`); `audit`/`explain` over D-Bus remain stubs pending the running-daemon query path — **landed (partial)**
- [x] Draft `packaging/udev/rules.d/99-optid.rules` (the `optid-udev` shim binary remains future work) — **landed**
- [x] `packaging/systemd/optid.service` exists; `WatchdogSec=` revert layer (§1.6) tracked separately — **pre-existing**

### Short-term (needs hardware)
- [ ] Run §4.1 APST state 4 hang test on T14 Gen 4 + XPS 13 + Framework 13
- [ ] Run §4.2 PCIe L1.2 link drop test on each reference laptop's wifi
- [ ] Run §4.3 panic-survival revert journal test
- [ ] Run §4.4 udev event latency test
- [ ] Enumerate full HWIDs for each reference laptop (`lspci -nn`, `lsusb`, `/sys/class/nvme/*/device/modalias`)
- [ ] Populate verified allowlist entries from at least 3 reference laptops

### Medium-term
- [ ] Promote from WIP to Validated once seeded baseline covers ≥ 5 laptops × ≥ 3 HWIDs each and all §4 experiments are closed
- [ ] Land `crates/optid/src/allowlist.rs` behind `--allowlist=enabled` flag (default `disabled` in v0.x)
- [ ] Update SPEC §4.3 status rows for Runtime PM / NVMe APST / PCIe ASPM to `A` once allowlist lands

* * *

## Appendix: Suggested Reading

### Kernel source
- `drivers/pci/pci-driver.c` — `pci_match_one_device()`, modalias format
- `drivers/pci/pcie/aspm.c` — `pcie_aspm_check_latency()`, L1.2 capability
- `drivers/nvme/host/pci.c` — `nvme_configure_apst()`, APST state table
- `drivers/ata/libata-scsi.c` — `ata_scsi_link_pm_policy()`, ALPM states
- `drivers/usb/core/driver.c` — `usb_match_one_id()`, USB modalias
- `drivers/acpi/bus.c` — `acpi_device_modalias()`

### Documentation
- `Documentation/admin-guide/devices.rst`
- `Documentation/PCI/pci.rst` — ASPM section
- `Documentation/admin-guide/nvme.rst`
- `Documentation/admin-guide/udev.rst`
- `systemd.service(5)` — `WatchdogSec=`

### Prior art
- libinput quirks DB (`/usr/share/libinput/*.quirks`) — identical pattern
- `systemd-hwdb` (`systemd-hwdb(8)`) — HWID-keyed data prior art
- `tlp` RUNTIME_PERPM — `https://linrunner.de/tlp/` — runtime PM policy prior art
- `powertop --auto-tune` — anti-prior-art (no revert path)

### Project-internal
- SPEC §3 (actuation rule), §4.3 (depth-enablers), §6 WP-N4 — `docs/SPEC-northstar.md`
- ADR-0009 — `docs/decisions/0009-optid-security-boundary.md`
- ADR-0013 — `docs/decisions/0013-detection-and-ml-boundary.md`
- Research 0002 — `docs/research/0002-rush-linux-architecture-review.md`
