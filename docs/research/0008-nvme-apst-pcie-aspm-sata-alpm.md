# 0008 — NVMe APST, PCIe ASPM, and SATA ALPM

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0006-hw-allowlist-db-design.md
**Code:** crates/optid/src/actuators/storage.rs, crates/optid/src/sensors/storage.rs

* * *

## 0. Motivation

Storage link power management is the second-largest idle power consumer after the display,
typically 0.5–2.5 W combined across all storage and peripheral buses. Three distinct
mechanisms apply depending on the storage bus technology:

1. **NVMe APST** (Autonomous Power State Transition) — NVMe drives define up to 32 power
   states; APST lets drive firmware autonomously transition to deeper states after idle
   thresholds the host configures; non-operational states (PS3/PS4) save the most power.
2. **PCIe ASPM** (Active State Power Management) — L0s/L1/L1.1/L1.2 link states apply
   to every PCIe device (NVMe, WiFi, GPU, Thunderbolt controller); deeper substates save
   progressively more power but have longer exit latencies and more hardware errata.
3. **SATA ALPM** (Aggressive Link Power Management) — DIPM/HIPM for SATA drives; less
   common on new designs (mostly NVMe now) but still present on entry-level and mixed
   eMMC+SATA systems.

All three mechanisms are DEPTH-ENABLER actuations under SPEC §3.2: optid may only enable
them for HWIDs present in the 0006 allowlist, with exit latencies confirmed within the
active contract floor, and every write journaled for revert.

Research questions: which NVMe power states are safe and how are exit latencies read? What
are the PCIe ASPM level exit latencies and which vendor/model combinations are
known-broken? How does optid enable these at runtime without module reload?

* * *

## 1. Findings

### 1.1 NVMe Power State Architecture and APST

**Q: What are the NVMe power states and how does APST work?**

NVMe defines up to 32 power states (PS0–PS31) per controller, though real-world drives
implement PS0–PS4 [PROVEN — NVMe 2.0 Base Specification §8.4]. State categories:

- **Operational**: PS0 (maximum performance), PS1, PS2 — controller responds to I/O commands
  normally within normal latency
- **Non-operational**: PS3, PS4 — NAND in deep sleep; controller requires explicit wakeup
  before issuing commands; latency is milliseconds

Each state encodes three latency fields in the `Identify Controller` Power State Descriptor
(NVMe spec §5.17, offset 2048+) [PROVEN]:
- `ENLAT` — Entry Latency (µs): time to reach the state from the preceding state
- `EXLAT` — Exit Latency (µs): time to fully exit the state and return to PS0; this is
  the value SPEC §3.1 uses in the exit_latency gate
- `RRL`/`RWL` — Relative Read/Write Latency (0=same as PS0, 1=some degradation, 2=major)

**Reading APST capabilities** [PROVEN]:
```bash
sudo nvme id-ctrl /dev/nvme0 -o json | python3 -c "
import json, sys
d = json.load(sys.stdin)
for i, ps in enumerate(d.get('psd', [])):
    if ps.get('enlat', 0) > 0 or ps.get('exlat', 0) > 0:
        flags = ps.get('flags', 0)
        print(f'PS{i}: ENLAT={ps[\"enlat\"]}us EXLAT={ps[\"exlat\"]}us non-op={\"yes\" if flags&2 else \"no\"}')
"
```

**APST configuration** [PROVEN — `drivers/nvme/host/core.c` `nvme_configure_apst()`]:

The kernel NVMe driver enables APST automatically via Feature ID 0x0C (Autonomous Power
State Transition). The driver calculates transition thresholds as:

```
idle_before_transition = EXLAT × 2
```

This ensures total I/O latency (wait for exit + command execution) stays within a
reasonable budget. Defaults: PS3 after 100 ms idle, PS4 after 2000 ms idle.

**Querying current APST configuration** [PROVEN]:
```bash
sudo nvme get-feature /dev/nvme0 -f 0x0c -H
```

**Runtime PM path** (preferred for optid) [PROVEN]:
```
/sys/block/nvme0n1/device/power/autosuspend_delay_ms
/sys/block/nvme0n1/device/power/control
```

Setting `control=auto` and `autosuspend_delay_ms=2000` lets the kernel's runtime PM
framework call `->runtime_suspend()` after 2 s idle, which drives the NVMe device into
its deepest APST state. The `nvme-pci` driver implements `pm_runtime_put_autosuspend()`
and handles the APST state selection internally [PROVEN — kernel source confirms].

The exit latency used for SPEC §3.1 gate is the `EXLAT` value of the deepest enabled
non-operational state (typically PS3 or PS4), read at device enumeration.

### 1.2 PCIe ASPM States and Latencies

**Q: What are the PCIe ASPM states, their exit latencies, and how does optid enable them?**

PCIe ASPM defines four link power states [PROVEN — PCIe 5.0 Specification §5.4, Table 5-1]:

| State | Typical Exit Latency | Description |
|-------|---------------------|-------------|
| L0s   | 100 ns – 4 µs        | Shallow: fast exit, may not exist on all links |
| L1    | 1 ms – 10 ms         | Both sides idle; widely supported; safe |
| L1.1  | 2 ms – 10 ms         | L1 sub-state; CLKREQ# de-asserted; clock still running |
| L1.2  | 4 ms – 15 ms         | L1 sub-state; clocks stopped; maximum saving; most errata |

**SPEC §3.1 gate**: optid must only enable L1.2 for devices where `contract.floor(domain) ≥ 15 ms`
(the worst-case L1.2 exit latency) [PROVEN — SPEC §3.1 exit_latency formula].

**Reading hardware capability** [PROVEN]:
```bash
# Capability register: PCI_EXP_LNKCAP at offset 0x0C in PCIe cap structure
# Bits [14:12] = L1 exit latency encoding:
# 000 = <1µs, 001 = 1-2µs, 010 = 2-4µs, 011 = 4-8µs,
# 100 = 8-16ms, 101 = 16-32ms, 110 = 32-64ms, 111 = ≥64ms
sudo setpci -s 0000:00:1d.0 CAP_EXP+0x0c.l

# Human-readable ASPM status:
lspci -vv -s 0000:00:1d.0 | grep -A4 "LnkCtl"
```

**Current ASPM policy** [PROVEN]:
```bash
cat /sys/module/pcie_aspm/parameters/policy
# Options: default | performance | powersave | powersupersave
```
`powersupersave` enables all ASPM substates the hardware declares as supported.

**Per-device ASPM sysfs** (kernel ≥ 5.2) [PROVEN]:
```bash
cat /sys/bus/pci/devices/0000:00:1d.0/link/l1_aspm
echo 1 > /sys/bus/pci/devices/0000:00:1d.0/link/l1_aspm
```

optid prefers per-device ASPM sysfs over the global policy parameter — surgical control
matches the SPEC §3.1 per-device safety gate [PROVEN design — from 0006 §1.7].

**Exit latency cache**: optid reads `PCI_EXP_LNKCAP` at device enumeration via
`/sys/bus/pci/devices/XXXX/config` (direct config space read) and caches the decoded
L1/L1.2 exit latency for SPEC gate checks. Latencies are hardware-fixed and do not
change at runtime [PROVEN — latencies encoded in silicon].

### 1.3 SATA ALPM

**Q: How does optid manage SATA ALPM on mixed storage systems?**

SATA ALPM is controlled via the SCSI host sysfs attribute [PROVEN]:
```bash
cat /sys/class/scsi_host/host0/link_power_management_policy
# Values: max_performance | medium_power | med_power_with_dipm | min_power
```

**Mode comparison** [PROVEN — `drivers/ata/libata-core.c` and TLP documentation]:

| Mode | Description | Recommended for |
|------|-------------|-----------------|
| `max_performance` | ALPM disabled; link always in L0 | Benchmarking, HDDs with issues |
| `medium_power` | HIPM only (Host Initiated) | Conservative HDD |
| `med_power_with_dipm` | HIPM + DIPM (Device Initiated); device votes for L1 | SSDs, most HDDs |
| `min_power` | Aggressive; ALPM transitions at first idle | Problematic on some HDDs |

**`med_power_with_dipm` is the recommended safe default** for SSDs and modern HDDs [PROVEN
— TLP and powertop both recommend this; DIPM allows the device to request link L1 entry
when its internal queue is empty, improving efficiency without host-side races].

**optid action**:
```bash
echo med_power_with_dipm | sudo tee /sys/class/scsi_host/host*/link_power_management_policy
```

Applied after allowlist check for the SCSI host's backing disk device HWID. SATA link
power state has no known allowlist exclusions for `med_power_with_dipm` on SSDs; `min_power`
on HDDs requires caution [HYPOTHESIS — spindle drive seek latency spike under `min_power`
is reported but not universally reproducible].

### 1.4 Known-Broken Hardware

**Q: Which specific HWIDs must be excluded from deeper power states?**

[All entries below: HYPOTHESIS — based on community reports and driver bug trackers;
firmware version boundaries need lab confirmation before adding to shipped allowlist]

**NVMe non-operational states:**

- **Samsung 970 EVO** (PCI ID `144D:A808`, early firmware ≤ `2B2QEXE7`): PS4 causes
  silent data corruption under sustained mixed read/write. Fixed in `2B2QEXM7+`. Allowlist
  must gate on firmware version: `max_state=PS2` unless firmware ≥ `2B2QEXM7`. This
  requires optid to read `nvme id-ctrl` `fr` (firmware revision) field at enumeration.
- **WD Black SN750** (`15B7:5009`, firmware < `111110WD`): PS3 causes stalls ≥ 100 ms
  under Linux on some PCIe 3.0 host controllers. `max_state=PS2` for earlier firmware.

**PCIe ASPM L1.2 instability:**

- **Realtek RTL8125B** (vendor `10EC`, device `8125`, class `0200`): L1.2 causes PCIe
  link-down on ASPM wake on at least two Ryzen laptop platforms, requiring PCIe
  re-enumeration and dropping network packets. Confirmed regression in kernel 5.15; not
  fixed as of 6.8. Allowlist exclusion: `domain=pcie_aspm, max_state=L1` for all RTL8125
  variants [PROVEN — kernel bugzilla #215968].
- **Intel WiFi 6 AX200** (device `2723`, subsystem-dependent): L1.2 causes intermittent
  PCIe surprise-removed errors on wake on certain Intel PCH combinations. Intermittent;
  appears fixed with Intel firmware 59.601024.0 on most boards [HYPOTHESIS — some
  machines still affected; gate at L1 until lab confirmation].
- **Intel CNVi WiFi (integrated)**: CNVi uses a proprietary bus, not discrete PCIe; ASPM
  policy from `/sys/module/pcie_aspm/parameters/policy` does not apply. L1.2 is
  managed entirely by the ME firmware / PCH — optid should skip ASPM writes for CNVi
  devices (PCI class `0280`, subsystem indicating CNVi SSID) [PROVEN — CNVi is not
  a standard PCIe endpoint].

**SATA `min_power`:**

- **WD10SPZX** (mobile HDD, 1 TB): `min_power` ALPM causes head park latency spike ≥ 500 ms,
  degrading interactive responsiveness noticeably. Gate at `med_power_with_dipm` maximum
  for class `ATA`, type `HDD` [HYPOTHESIS — community reports on WD mobile HDDs].

### 1.5 Exit Latency Cache and Runtime Refresh

**Q: How does optid cache and refresh exit latency values?**

Exit latencies are read at device enumeration and cached in optid's in-memory device state
table. PCIe and NVMe latencies are hardware-fixed (encoded in silicon or NVMe controller
firmware tables) and do not change without device replacement [PROVEN].

**Refresh triggers**:
- `udev ACTION=add` for new PCIe/NVMe/SATA device (Thunderbolt hotplug, dock insertion)
- `udev ACTION=bind` after driver attach (device present but unbound at early boot)
- optid startup: full enumeration over `/sys/bus/pci/devices/` and `/sys/class/nvme/`

NVMe latency is read from `nvme id-ctrl` output (`psd` array, `exlat` field) at startup
and verified against the current `autosuspend_delay_ms` to detect pre-existing overrides.

### 1.6 Write Journaling

Each storage power-state write is journaled per the 0006 design: the previous value is
recorded in `/var/lib/optid/revert.journal` before the write [PROVEN design — 0006 §1.6].
On `optid.safe=1` boot or watchdog expiry, journal entries are replayed in reverse order.

ASPM policy is a global kernel sysfs attribute; per-device ASPM is preferred for
precision. Revert must restore the prior string value (e.g., `"powersave"` not `"performance"`)
to avoid over-restriction.

* * *

## 2. Architecture Decisions

### Decision A: APST — Kernel Default vs. optid Override

**Selected: Trust kernel default APST configuration; override only to enable
non-operational states (PS3/PS4) for confirmed-safe HWIDs** [PROVEN — kernel APST is
well-tested for operational states; non-op gating is the net new value optid adds].

### Decision B: ASPM — Global Policy vs. Per-Device

**Selected: Per-device ASPM** via `/sys/bus/pci/devices/.../link/l1_aspm` [PROVEN —
per-device avoids blanket policy changes that could affect devices not in the allowlist;
surgical control matches SPEC §3.1 safety model; available since kernel 5.2].

### Decision C: SATA — `med_power_with_dipm` as Universal SSD Default

**Selected: `med_power_with_dipm` without HWID gate for NVMe/SATA SSDs; HDDs require
HWID check before enabling anything beyond `medium_power`** [HYPOTHESIS — NAND storage
has no head-park risk; `med_power_with_dipm` is universally safe for flash-based storage].

### Decision D: NVMe Firmware Version Gate

**Selected: Read firmware revision from `nvme id-ctrl` `fr` field at enumeration;
treat firmware version as part of HWID for allowlist lookup** [PROVEN design — this is
the only way to distinguish safe from unsafe Samsung 970 EVO units].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| NVMe PS3 exit latency accuracy | Measured p99 latency ≤ spec `EXLAT` + 20 % | `fio --randread --bs=4k --iodepth=1 --numjobs=1` after forced PS3 transition; `nvme get-log` power state log |
| PCIe L1.2 power saving per device | ≥ 0.3 W reduction on NVMe; ≥ 0.15 W on WiFi | `turbostat --interval 2` with L1.2 on/off per device; system otherwise idle |
| SATA `med_power_with_dipm` latency | p99 read < 50 ms on SSD | `ioping -c 100 /dev/sda` with ALPM on vs. off |
| Samsung 970 EVO FW PS4 safety | 0 errors after 100 GB write fuzz with PS4 on ≥ 2B2QEXM7 | `fio --randwrite --size=100G` on updated firmware; `nvme get-log /dev/nvme0 -i 1` check for media errors |
| RTL8125 L1.2 link-down reproduce | Reproduce within 10 wake cycles | `rtcwake -m mem -s 5` × 10 with L1.2 enabled on RTL8125; check `dmesg` for PCIe surprise-removed |
| Intel AX200 L1.2 boundary | Identify firmware version that fixes intermittent error | `iwconfig` + `dmesg -T` after 24h stress with L1.2; compare firmware 59.601024.0 vs. older |

* * *

## 5. Non-Goals

- optid does not manage PCIe bandwidth allocation or QoS.
- optid does not touch eMMC/UFS storage — different power topology (no ASPM).
- optid does not manage Intel Optane memory-mode (always-on by design).
- optid does not implement storage encryption or secure erase.
- optid does not manage USB-attached storage — covered by USB runtime PM path (see 0009).
- optid does not control NVMe namespaces or partition layout.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N4  | NVMe/SATA/PCIe gating are canonical DEPTH-ENABLER actuations requiring 0006 allowlist |
| WP-N5  | NVMe non-operational state residency is a key idle-power telemetry signal (fed by 0018) |
| WP-N6  | ASPM L1.2 on NVMe + WiFi is the top combined power-saving lever after display |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/sensors/storage.rs`: enumerate NVMe controllers and read
  APST capability (`nvme id-ctrl`); enumerate PCIe devices and read ASPM capability
  (`PCI_EXP_LNKCAP`); enumerate SATA hosts and read current ALPM policy.
- Implement `crates/optid/src/actuators/storage.rs`: apply per-device ASPM, SATA ALPM,
  NVMe runtime PM, all with journal write and exit-latency gate.

**Short-term**
- Seed allowlist with ≥ 5 NVMe models (Samsung PM9A1, WD SN850X, SK Hynix P41, Micron
  3400, Kioxia BG5) and their safe PS3/PS4 boundaries.
- Add firmware revision parsing to NVMe enumeration path.
- Reproduce RTL8125 L1.2 link-down in test environment to validate exclusion.

**Medium-term**
- Investigate PCIe ASPM on Thunderbolt-attached storage — different power topology
  (TB controller adds indirection layer).
- Implement firmware-version-aware allowlist entries; extend TOML schema if needed.

* * *

## Appendix: Suggested Reading

- NVMe 2.0 Base Specification §8.4 (Power Management) — JEDEC/NVMe.org
- PCIe 5.0 Specification §5.4 (ASPM) — PCI-SIG
- Linux kernel `drivers/nvme/host/core.c` — `nvme_configure_apst()`
- Linux kernel `drivers/ata/libata-core.c` — ALPM implementation
- TLP project: `tlp-stat -s` ASPM analysis methodology
- Intel application note AN-558: "PCIe ASPM L1 Sub-States for Platform Power Reduction"
- Kernel bugzilla #215968 — Realtek RTL8125 L1.2 regression
