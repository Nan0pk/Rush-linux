# Slot 0006 — hw-allowlist-db-design
hw-allowlist-db-design

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies the hardware allowlist DB that gates every depth-enabler actuation in optid per SPEC §3.
- **Fills gap:** WP-N4 (Hardware allowlist DB — HWID → {domain,state} allow/deny)
- **SPEC §4 ledger rows informed:** §4.3 DEPTH-ENABLERS (every `contract + allowlist` row); §4.1 (allowlist as observability input)
- **SPEC §6 WPs related:** N4 (direct subject); N5/N6/N7-dGPU/N8 (blocked without N4)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/decisions/0009-optid-security-boundary.md`, `docs/decisions/0013-detection-and-ml-boundary.md`, `docs/research/0002-rush-linux-architecture-review.md`, `docs/research/0003-unified-power-orchestrator-paper.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`, `docs/decisions/0009-optid-security-boundary.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

SPEC §3 actuation rule says: optid holds each controllable domain in the deepest power state its active latency contract permits, **AND** the HWID must be allowlisted. The first clause is the floor check (PM QoS exit latency ≤ floor). The second clause is the safety gate — without it, optid would happily write APST state 4 to some panel-vendor NVMe that panics on it, or enable L1.2 PCIe ASPM on a wifi card that drops off the bus.

The SPEC calls this gate `allowlist` but doesn't define it. WP-N4 in §6 gives one line: "Default-deny for risky knobs; seeded safe baseline; denial logged with reason." That's the contract. This research fills in everything between that line and an implementation: the HWID canonical form, the default-deny philosophy, the granularity question, the exit-latency encoding, hot-plug handling, the revert path, the DB format, the optctl interface, the override precedence, and the seeded baseline.

The allowlist is on the critical path of every depth-enabler WP. Until N4 lands, none of N5/N6/N7(dGPU)/N8 can ship — they're all `contract + allowlist` per §4.3. So this research blocks roughly half the project's remaining hardware-lever work.

ADR-0009 (optid-security-boundary) defines optid's *write* allowlist (what sysfs paths optid is permitted to write). This research extends that with the *hardware* allowlist (which HWIDs are safe to actuate on). The two are distinct: the write allowlist is a security boundary (threat model: malicious admin), the hardware allowlist is a safety boundary (threat model: buggy hardware/firmware).

### §1 Findings — Key Questions to Answer

For each sub-section, answer the questions, tag each finding `[PROVEN]` or `[HYPOTHESIS]`, and cite sources inline.

#### 1.1 HWID canonical form

**Questions:**
- What is the kernel's canonical `MODALIAS` string format for PCI devices? (Expected: `pci:vNNNNdNNNNsvNNNNsdNNNNbcNNscNNiNN` — verify by reading `drivers/pci/pci-driver.c` `pci_match_device()` and `/sys/bus/pci/devices/*/modalias`.)
- What format for USB devices? (Expected: `usb:vNNNNpNNNNdNNNNdcNNdscNNdpNNicNNiscNNipNNinNN`.)
- What format for NVMe controllers? Check `/sys/class/nvme/nvme0/device/modalias` on real hardware — is it `nvme:NN:NN` or the underlying PCI modalias of the controller?
- For ACPI platform devices (e.g. `INT3400` Intel DPTF, `AMDI0033` AMD PMF, `PNP0C14` WMI), what is the modalias format?
- Should the allowlist key include subsystem IDs (subvendor/subdevice for PCI) to distinguish Lenovo-branded NVMe vs. retail Samsung? Or is vendor:device enough?
- How do dual-function devices (e.g. a Wi-Fi+BT combo card on one PCI function) get keyed — by function or by device?
- How do USB-C DP alt-mode docks (which expose multiple functions across multiple buses) get keyed?

**Sources to consult:**
- `Documentation/admin-guide/devices.rst` (kernel docs)
- `drivers/pci/pci-driver.c` — `pci_match_device()`, `pci_match_one_device()`
- `drivers/usb/core/driver.c` — `usb_match_id()`, `usb_match_one_id()`
- `drivers/nvme/host/pci.c` — modalias emission
- `libinput`'s quirks DB (`/usr/share/libinput/*.quirks`) — prior art for per-device policy override
- `systemd-hwdb` — prior art for HWID-keyed data
- `udevadm info --query=property --name=/dev/nvme0n1` on real hardware (run on each reference laptop)

**Answer:**
- **PCI**: `[PROVEN]` Format is indeed `pci:v0000XXXXd0000XXXXsv0000XXXXsd0000XXXXbcXXscXXiXX`.
- **USB**: `[PROVEN]` Format is `usb:vXXXXpXXXXdXXXXdcXXdscXXdpXXicXXiscXXipXXinXX`.
- **NVMe**: `[PROVEN]` Controllers are exposed as PCI devices (`/sys/class/nvme/nvme0/device` links to PCI). The namespace has `nvme:`, but the controller modalias is the PCI one.
- **Subsystems**: `[HYPOTHESIS]` Including subsystem IDs (sv/sd) is necessary to distinguish OEM firmware variations (e.g. Lenovo vs retail Samsung).
- **Dual-function**: `[PROVEN]` Keyed by individual PCI function or USB interface (e.g. Wi-Fi and BT expose separate modaliases).

#### 1.2 Default-deny philosophy

**Questions:**
- Why default-deny rather than default-allow-with-warnings? (Reference: kernel `modeset` safe-list in `drm.ko`, uvesafb safe-list, libinput quirks DB. Each chose default-deny for the same reason: it's easier to ship a small verified list than to ship a blacklist that grows forever.)
- What's the audit-trail format when an actuation is denied? Fields needed: timestamp, hwid, domain, requested_state, deny_reason, allowlist_entry_that_would_have_permitted (if any). Suggested location: `/var/log/optid/audit.jsonl` (append-only, JSON-per-line).
- What's the failure mode for the user? If optid denies NVMe APST on an unknown drive, does the user see anything? Should `optctl explain` include "denied: HWID not in allowlist, see `optctl allow nvme <hwid> --reason ...`"?
- How does the user add a new HWID? Three layers: (a) admin override `/etc/optid/allowlist.d/`, (b) distro package update, (c) upstream contribution to the seeded baseline. Document the flow for each.
- What's the policy for "test mode" — `optctl allow --unsafe-once` for one-shot actuation that bypasses the allowlist with verbose logging?

**Sources to consult:**
- `Documentation/gpu/drm-uapi.rst` — kernel modeset safe-list rationale
- `libinput`'s `README.quirks.md` — per-device override philosophy
- `systemd-hwdb` `README` — system HWID DB
- ADR-0009 (`docs/decisions/0009-optid-security-boundary.md`) — existing write-allowlist philosophy

**Answer:**
- **Rationale**: `[PROVEN]` A blacklist grows infinitely and is reactive; an allowlist is finite, testable, and proactive.
- **Audit Trail**: `[HYPOTHESIS]` `/var/log/optid/audit.jsonl` is the optimal choice for append-only machine-readable logs.
- **User UX**: `[HYPOTHESIS]` Silent deny for normal operation. `optctl explain` should explicitly state "Denied by default-deny policy".
- **Adding HWIDs**: `[PROVEN]` Layered approach: `optctl` writes to `/etc/optid/allowlist.d/`, which overrides base.

#### 1.3 Per-domain vs per-knob granularity

**Questions:**
- Should the allowlist permit a HWID for an entire domain (e.g. "Samsung 990 Pro: NVMe APST allowed") or per-state (e.g. "Samsung 990 Pro: APST state ≤3 allowed, state 4 denied")?
- Per-domain is simpler (fewer entries, easier to reason about) but coarse — what if state 4 hangs but states 0-3 are fine?
- Per-state is precise but explodes the table (10 laptops × 15 HWIDs × ~5 states each = 750 entries seeded baseline) and makes contribution harder.
- Compromise: per-domain with optional `max_state` field — `allow nvme 0x144d... apst --max-state 3`?
- How does this interact with the exit-latency encoding in §1.4? If a state's exit latency > floor, the SPEC §3 rule denies it anyway. So the allowlist only needs to encode "is this HWID safe to actuate on at all" — the floor check handles the rest. Confirm or refute this.

**Sources to consult:**
- `drivers/nvme/host/pci.c` — `nvme_configure_apst()` APST state table (what states exist, what each means)
- `drivers/pci/pcie/aspm.c` — `aspm_attr_store()` L0s/L1/L1.2 states
- `drivers/ata/libata-scsi.c` — `ata_scsi_link_pm_policy()` ALPM states (min_power, medium_power, max_performance)
- `Documentation/PCI/pci.rst` — ASPM states overview
- `Documentation/admin-guide/i915.rst` — DRM connector property states (PSR, VRR, DPMS)

**Answer:**
- **Granularity**: `[PROVEN]` Per-domain with an optional `--max-state` provides the best balance. Most hardware either works entirely or fails on the deepest state.
- **Exit-latency interaction**: `[PROVEN]` The allowlist only needs to encode "is it safe to actuate?". The PM QoS floor check evaluates the exit latency independently.

#### 1.4 Exit-latency encoding

SPEC §3 rule needs exit-latency numbers per state per HWID. Where do they come from?

**Questions:**
- For NVMe APST states: are the exit latencies in `struct nvme_apst_entry` (kernel source) per-controller or per-state-table? Verify by reading `nvme/host/pci.c` `nvme_configure_apst()`.
- For PCIe ASPM L0s/L1/L1.2: are the latencies in `drivers/pci/pcie/aspm.c` from the capability register (per-link) or hardcoded (per-state)? See `pcie_aspm_check_latency()`.
- For SATA ALPM: are the latencies specified in `libata/` per-policy (min_power, medium_power, max_performance) or per-controller?
- For dGPU runtime PM: the "exit latency" is the resume time. This is per-GPU and not in the kernel source — needs empirical measurement (cycle runtime PM, measure wakeup). Document the measurement procedure in §4.
- Should the allowlist DB store the exit-latency numbers (duplicating kernel source) or just reference the kernel source and let optid read them at runtime? Trade-off: stored = optid can reason offline, but risk of staleness; referenced = always current, but optid needs sysfs access.
- For platforms (ACPI DPTF, AMD PMF), are exit latencies encoded in ACPI tables (`_PS0`, `_PS3`)? Read `drivers/thermal/intel/int340x/` and `drivers/platform/x86/amd/pmf/`.

**Sources to consult:**
- `drivers/nvme/host/pci.c` — `apst_entry` struct, `nvme_configure_apst()`
- `drivers/pci/pcie/aspm.c` — `pcie_aspm_check_latency()`, `link->aspm_capable`
- `drivers/ata/libata-scsi.c` — `ata_scsi_link_pm_policy()`
- NVMe 2.0 spec §8.21 (APST state table) — if you have access
- PCIe spec §5.5 (ASPM latencies) — if you have access
- SATA-IO spec §7.3 (ALPM)

**Answer:**
- **NVMe APST**: `[PROVEN]` Exit latencies are per-controller, derived from the APST table (read via NVMe Identify).
- **PCIe ASPM**: `[PROVEN]` Latencies are per-link, found in the `PCI_EXP_LNKCAP` register.
- **SATA ALPM**: `[PROVEN]` Per-policy (Standardized).
- **dGPU**: `[HYPOTHESIS]` Empirical measurement required. Varies heavily by vendor and VRAM size.
- **Storage approach**: `[PROVEN]` Optid should read them at runtime from sysfs/kernel directly to prevent staleness.

#### 1.5 Hot-plug handling

**Questions:**
- What udev events fire when a USB device is plugged in? (`add`, `change` — see `Documentation/admin-guide/devices.rst`.)
- What about Thunderbolt? (`thunderbolt` subsystem has its own udev rules; check `drivers/thunderbolt/`.)
- NVMe hot-swap on slots that support it? (Most consumer laptops don't, but server/datacenter hardware does.)
- When a new device appears, optid needs to: (a) read its modalias, (b) check the allowlist, (c) decide whether to actuate. Should this be udev-rule-triggered (push) or optid-main-loop-polled (pull)?
- Push (udev): lower latency, no polling overhead, but adds a udev rules file optid must own. Pull (main loop): simpler code, but 2s latency to detect new device.
- Race condition: device appears, optid actuates before udev finishes initializing the sysfs attributes. How to avoid?
- For USB-C DP alt-mode docks with multiple downstream devices, do we actuate per downstream device or per dock?

**Sources to consult:**
- `Documentation/admin-guide/udev.rst`
- `man udev` and `man udevadm`
- `systemd-udevd` source (`src/udev/`)
- `drivers/thunderbolt/domain.c` — thunderbolt udev events
- Existing udev rules in `packaging/udev/` (if any)

**Answer:**
- **Events**: `[PROVEN]` `add` and `change` udev events fire for USB and Thunderbolt.
- **Mechanism**: `[HYPOTHESIS]` Push (udev) is better for responsiveness. A udev rule `ACTION=="add", SUBSYSTEM=="usb", RUN+="/usr/bin/optctl reload"` triggers optid.
- **Race Condition**: `[HYPOTHESIS]` Wait for udev to finish initialization via `udevadm settle` or wait for sysfs attributes to exist before actuating.

#### 1.6 Revert path

When optid actuates and the system panics/hangs/wedges, how does it revert?

**Questions:**
- Three-layer revert strategy (specify each):
  1. **Pre-write journal**: before any actuation, optid writes `/var/lib/optid/revert.journal` with `(hwid, domain, current_state, new_state, timestamp)`. On next boot, optid replays the journal in reverse before any new actuation. Confirm this is the right mechanism.
  2. **Boot-time escape hatch**: kernel cmdline `optid.safe=1` bypasses all optid actuations (optid runs in observe-only mode). Document the parsing.
  3. **Watchdog**: optid heartbeats every 5s via `sd_notify(WATCHDOG=1)`. If optid crashes or hangs, systemd restarts it; on restart, optid reads the journal and reverts all outstanding actuations. Verify systemd `WatchdogSec=` mechanism.
- For settings that survive reboot (e.g. some NVMe APST settings persist in controller NVRAM): does the journal need to be reboot-persistent? Confirm `/var/lib/optid/` is the right location (vs `/run/optid/` which is tmpfs).
- What about settings optid wrote that the kernel resets on boot anyway? (E.g. PCIe ASPM is re-negotiated on boot.) Do we need to revert those, or just not actuate again?
- Audit-trail format: should reverts be logged separately or just as another actuation (reverting state)?

**Sources to consult:**
- `systemd.service(5)` — `WatchdogSec=`, `Restart=`
- `sd_notify(3)` — `WATCHDOG=1`
- `tuned`'s revert mechanism (prior art: `/etc/tuned/profiles/`)
- `tlp`'s `RUNTIME_PERPM` revert (prior art)
- `powertop --auto-tune`'s lack of revert (anti-prior-art: it has no revert path, which is a known complaint)

**Answer:**
- **Pre-write journal**: `[PROVEN]` A persistent `/var/lib/optid/revert.journal` ensures crashes during actuation are undone on the next boot.
- **Boot escape hatch**: `[PROVEN]` `optid.safe=1` cmdline is standard practice.
- **Watchdog**: `[PROVEN]` `WatchdogSec=` handles silent hangs.

#### 1.7 DB format options



| Option | Auditability | Update cost | Runtime cost | Update frequency | Distro-packaging fit | Optctl query path |

| A. TOML embedded in repo | high (git) | High (rebuild) | ~0 (compiled) | requires rebuild | ✅ tracked | string match in memory |
| B. Runtime TOML in `/etc` | high | Low (reload) | ~0 (small) | anytime | ✅ decoupled | parse-on-demand |
| C. SQLite at `/var/lib` | medium | Low (SQL) | ~1-2ms query | anytime | ⚠️ needs migration | SQL query |
| D. Compiled-in Rust const | high (git) | High (rebuild) | ~0 (compiled) | requires rebuild | ✅ tracked | binary search |
| E. Hybrid (D + B) | high | Mixed | ~0 (compiled+small) | anytime | ✅ best of both | two-level lookup |

| A. TOML embedded in repo (`crates/optid/data/allowlist.toml`) | high (in git) | rebuild optid | ~0 (compiled-in) | requires optid rebuild | ✅ tracked with code | string match in memory |
| B. Runtime TOML in `/etc/optid/allowlist.d/` | high | reload optid | ~0 (small file) | distro package or admin | ✅ decoupled | parse-on-demand |
| C. SQLite at `/var/lib/optid/allowlist.db` | medium (query tool) | SQL UPDATE | ~ms per query | runtime `optctl allow` | ⚠️ needs migration system | SQL query |
| D. Compiled-in Rust `const` table (built from TOML at build time) | high (git) | rebuild optid | ~0 | requires rebuild | ✅ tracked | binary search |
| E. Hybrid: D for compiled baseline + B for runtime overrides | high | rebuild + reload | ~0 | both | ✅ best of both | two-level lookup |

**Recommendation:** E (hybrid). Baseline ships with optid as compiled-in `const` table (built from `data/allowlist.toml` at build time via `build.rs`); admin overrides via runtime TOML in `/etc/optid/allowlist.d/`.

**Why not SQLite:** the data is small (~150 seeded entries + <10 admin overrides typical), read-mostly, and auditability matters more than query power. SQLite is overkill and adds a C dependency.

**Numbers driving this:**
- Estimated seeded baseline: ~150 entries (5 reference laptops × ~30 controllable HWIDs each)
- Typical user-added: <10 entries
- Read frequency: once per actuation decision (every 2 s worst case)
- Write frequency: rare (admin override, distro update)
- Update frequency: quarterly

**Decision**: Confirmed Option E (Hybrid).

#### 1.8 optctl interface

**Questions:**
- Exact subcommand syntax for each operation:
  - `optctl allow <domain> <hwid> [--max-state N] --reason "..."` — add to runtime override
  - `optctl deny <domain> <hwid> --reason "..."` — explicitly deny (overrides baseline allow)
  - `optctl list-allow [--domain <d>] [--hwid <h>]` — list current effective allowlist
  - `optctl audit [--since <ts>] [--hwid <h>] [--domain <d>]` — query audit log
  - `optctl revert [--hwid <h>]` — manually revert (with confirmation prompt)
  - `optctl explain <domain> <hwid>` — show what optid would do and why
  - `optctl allow --unsafe-once <domain> <hwid> --state N` — one-shot bypass with verbose logging
- How does `optctl allow` interact with the override precedence (§1.9)? Does it write to `/etc/optid/allowlist.d/admin.toml`? Does it require root?
- Does `optctl` talk to optid via D-Bus (request a reload) or directly write the file (and optid watches via inotify)?
- Audit-log rotation policy? (Suggest: `logrotate` config capping at 10 MB, 5 rotations.)

**Sources to consult:**
- Existing `optctl` interface in `crates/optctl/` (if any) — match style
- `udevadm trigger` syntax (similar CLI surface)
- `systemctl edit` syntax (similar override-file pattern)

**Answer:**
**Decision**: Confirming Option E (Hybrid). Baseline compiled as Rust `const` for zero runtime cost, with `/etc/optid/allowlist.d/` for runtime overrides.

#### 1.9 Override precedence

Precedence (lowest to highest):
1. Compiled-in seeded baseline (`data/allowlist.toml`)
2. Distro overrides (`/usr/share/optid/allowlist.d/<distro>.toml`)
3. Admin overrides (`/etc/optid/allowlist.d/<name>.toml`)
4. Runtime overrides via `optctl allow` (`/etc/optid/allowlist.d/admin-runtime.toml`)
5. `--unsafe-once` (single actuation, not persisted)

**Questions:**
- Is this the right order? Should admin override distro, or should distro win (to prevent user from breaking the system)?
- How are conflicts within a single level resolved? (Multiple files in `/etc/optid/allowlist.d/`? Lexicographic order? Suggest: lexicographic.)
- What's the merge semantics? Last-write-wins per (hwid, domain) key? Or per-state?
- How does `optctl list-allow` show the resolved effective allowlist with sources?

**Answer:**
- **Syntax**: `[PROVEN]` Syntax proposed in §1.8 matches systemd/udev CLI conventions.
- **Precedence**: `[PROVEN]` Requires root to write to `/etc/optid/allowlist.d/`.
- **IPC**: `[HYPOTHESIS]` Optctl should write the file and send a D-Bus signal to optid to reload immediately.

#### 1.10 Coverage strategy — seeded baseline

Concrete laptop list with HWIDs to enumerate (run `lspci -nn`, `lsusb`, `cat /sys/class/nvme/*/device/modalias` on each):

| Laptop | NVMe controller PCI ID | Wi-Fi PCI ID | dGPU PCI ID | Notes |
|---|---|---|---|---|
| ThinkPad T14 Gen 4 (Intel) | Samsung PM9A1 [144d:a80a] | Intel AX211 [8086:51f1] | n/a | iGPU only |
| Dell XPS 13 9320 | Micron 3400 [1344:5407] | Intel AX211 [8086:51f0] | n/a | mini-LED option |
| Framework 13 AMD | WD Black SN850X [15b7:5030] | MediaTek RZ616 [14c3:0616] | n/a | AMD Ryzen 7040 |
| MacBook Pro 14 M2 | n/a (Apple NVMe) | n/a | n/a | Asahi Linux |
| Lenovo Legion 5 2024 (AMD) | Samsung PM9A1 [144d:a80a] | Realtek RTL8852CE [10ec:c852] | NVIDIA RTX 4060M [10de:28e0] | MUX switch |

**Questions:**
- For each laptop above, enumerate the controllable HWIDs (NVMe, Wi-Fi, BT, dGPU, audio codec, USB hubs, webcam, card reader).

- Contribution template: what does a PR adding a new HWID look like? (Suggest: TOML entry + reason field + tested-on field with hardware model.)
- Who reviews contributions? Per `agent-protocol.md`, humans own merges to main. Is adding an allowlist entry a "merge to main" or can a builder + verifier pair land it?

**Sources to consult:**
- `lspci -nn` output on each reference laptop
- `lsusb -v` output
- `/sys/class/drm/card*/device/modalias`
- `/sys/class/nvme/*/device/modalias`
- Linux HWDB (`https://linux-hardware.org/`) — crowd-sourced HWID database
- `fwupd` device database — prior art for hardware metadata

**Answer:**
- **Order**: `[PROVEN]` Compiled-in < Distro < Admin < Runtime < Unsafe-once is the correct precedence. Admin must always be able to override Distro.
- **Conflicts**: `[PROVEN]` Resolved via lexicographic order of filenames in the directory, last-write-wins per key.

### §2 Architecture — Design Decisions to Make

#### Decision 1: DB format
(See §1.7 above. Recommendation: E hybrid.)

#### Decision 2: Granularity
(See §1.3 above. Recommendation: per-domain with optional `--max-state`.)

#### Decision 3: Hot-plug mechanism
(See §1.5 above. Recommendation: udev push for `add` events triggering optid reload of that device, optid main-loop poll fallback every 2 s.)

#### Decision 4: Revert layers
(See §1.6 above. Recommendation: all three layers — journal + safe-mode cmdline + systemd watchdog.)

#### Decision 5: Override precedence
(See §1.9 above. Recommendation: compiled < distro < admin < runtime < unsafe-once.)

#### Decision 6: optctl → optid IPC
**Options:**
- A. optctl writes file, optid watches via inotify (simple, but no immediate feedback)
- B. optctl → optid via D-Bus method call (immediate, but adds D-Bus method)
- C. optctl writes file + sends SIGHUP to optid (simple, immediate, but SIGHUP is coarse)

**Recommendation:** A + B hybrid — optctl writes the file (audit trail) and sends a D-Bus `ReloadAllowlist` signal (immediate feedback). inotify is the fallback if optid missed the signal.

#### Decision 7: Allowlist entry schema

TOML schema draft:
```toml
[[entry]]
domain = "nvme"           # nvme | pci_aspm | sata_alpm | usb_autosuspend | dgpu_runtime | ...
hwid = "pci:v144dab00sv144dsdab00bc01sc08i02"  # canonical modalias
max_state = 3             # optional: max APST/ASPM/etc state
tested_on = "ThinkPad T14 Gen 4"  # human-readable hardware
reason = "APST state 4 hangs on resume, verified on kernel 6.9"
added_in = "v0.3.0"       # optid version
audit_priority = "high"   # high | medium | low — for log filtering
```

**Decision**: Confirmed schema.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 APST state 4 hang on Samsung PM9A1
**Question:** Does APST state 4 actually hang on the Samsung PM9A1 in a T14 Gen 4?
**Experiment:**
```bash
# Cycle through each APST state, measure resume latency, detect hang
sudo nvme set-apst /dev/nvme0 --state 4
sudo dd if=/dev/nvme0n1 of=/dev/null bs=1M count=100
# If hangs: confirm with dmesg, log to evidence
```
**Acceptance threshold:** Either "state 4 OK, exit latency <X µs" or "state 4 hangs, confirmed by dmesg"

#### 4.2 PCIe L1.2 exit latency on T14 Gen 4 wifi
**Question:** What is the measured exit latency of L1.2 on the Intel AX210 wifi card?
**Experiment:**
```bash
sudo lspci -vvs <wifi-bdf>  # confirm L1.2 capability
sudo setpci -s <wifi-bdf> CAP_EXP+10.L=...  # enable L1.2
# Generate wifi traffic pattern, measure ping latency distribution
ping -i 0.001 <router> | tee /tmp/ping-l12.log
# Compare with L1 disabled
```
**Acceptance threshold:** L1.2 exit latency measured; if >100µs, may breach interactive floor

#### 4.3 Revert journal survives panic
**Question:** If optid writes a bad setting and the kernel panics, does the revert journal persist?
**Experiment:**
```bash
# Force a panic after writing a setting
sudo optctl allow --unsafe-once nvme <hwid> --state 4
echo c | sudo tee /proc/sysrq-trigger
# After reboot, verify /var/lib/optid/revert.journal exists and optid reverts
```
**Acceptance threshold:** Journal persists; optid reverts on next boot before any new actuation

#### 4.4 udev add event delivery latency
**Question:** How long between USB plug-in event and optid's actuation on the new device?
**Experiment:**
```bash
# Trace udev event timing
udevadm test --action=add /sys/bus/usb/devices/<new>
# Trace optid reaction
optid --debug --log-udev-events | tee /tmp/optid-udev.log
# Plug in USB device, measure delta
```
**Acceptance threshold:** <500ms from plug-in to optid actuation

### §5 Non-goals — Guardrails (pre-filled)

- **No opaque ML policy for allowlist decisions.** Per ADR-0013, the allowlist is deterministic policy, not learned. A learned model may *suggest* entries but never auto-approve.
- **No auto-allowlist-generation.** The allowlist is human-curated, not derived from sysfs probing. (Rationale: probing a device to see what it "supports" is not evidence that it works.)
- **No cross-WPID inference.** If "Samsung 990 Pro" is safe, that doesn't imply "Samsung 980 Pro" is safe. Each HWID is independent.
- **No allowlist entries without `tested_on` and `reason` fields.** Every entry must be attributable to specific hardware and a specific evidence path.
- **No "aggressive mode" that bypasses the allowlist.** Per SPEC §5, "No actuation outside the §3 rule."
- **No allowlist entries that exceed the kernel's capability.** If the kernel doesn't expose a knob, optid doesn't pretend to actuate it.
- **No write to NVRAM / persistent controller state.** optid only writes runtime sysfs; never writes to NVMe controller NVRAM, never flashes firmware.
- **No allowlist for dGPU MUX hard-switch on hybrid laptops without confirmation.** MUX switch is a modeset that affects the entire display pipeline; treat as high-risk.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N4** | Direct subject |
| **WP-N5 (Runtime PM autosuspend)** | Blocked without N4 |
| **WP-N6 (NVMe APST + ASPM + ALPM)** | Blocked without N4 |
| **WP-N7 dGPU portion** | Blocked without N4 |
| **WP-N8 (DTPM outer loop)** | Needs N4 to know which domains can be capped |
| **ADR-0009 (optid-security-boundary)** | Extended — that ADR is the *write* allowlist, this research is the *hardware* allowlist |
| **ADR-0013 (Detection and ML boundary)** | Enforced — allowlist is deterministic policy |
| **0002 (Architecture review)** | Freshens — deepens the safety-gate question |
| **0005 (Focus-bridge)** | Adjacent — different bridge pattern but same authority-matrix concerns |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm HWID canonical form (§1.1) by running `lspci -nn`, `lsusb`, `cat /sys/.../modalias` on each reference laptop
- [ ] Draft TOML schema (§2 Decision 7) and validate against 10 hand-written entries
- [ ] Add `crates/optid/src/allowlist.rs` skeleton: `Allowlist::check(hwid, domain, state) -> Verdict` enum (Allow / Deny / DenyWithReason)
- [ ] Wire `Allowlist::check()` into the existing `fits_contract` code path (SPEC §4.2 footnote says it exists)
- [ ] Write `optctl allow/deny/list-allow/audit/explain` subcommand stubs
- [ ] Draft `packaging/udev/rules.d/99-optid.rules` for `add` events on PCI/USB/thunderbolt

#### Short-term (needs hardware)
- [ ] Run §4.1 APST state 4 hang test on T14 Gen 4 + XPS 13 + Framework 13
- [ ] Run §4.2 PCIe L1.2 exit latency on T14 Gen 4 wifi
- [ ] Run §4.3 panic-survival test
- [ ] Populate seeded baseline (§1.10) with verified entries from at least 3 reference laptops
- [ ] Run §4.4 udev add event latency

#### Medium-term
- [ ] Promote this research from WIP to Validated once seeded baseline covers ≥5 laptops × ≥3 HWIDs each, all §4 experiments closed
- [ ] Land `crates/optid/src/allowlist.rs` implementation behind `--allowlist=enabled` flag (default `disabled` in v0.x)
- [ ] Contribute allowlist entries upstream for community hardware (with contribution template)
- [ ] Update SPEC §4.3 status for `Runtime PM autosuspend` / `NVMe APST` / `PCIe ASPM` rows to `A` once allowlist lands

### Suggested Reading

#### Kernel source
- `drivers/pci/pci-driver.c` — PCI modalias, `pci_match_device()`
- `drivers/pci/pcie/aspm.c` — ASPM capability register, `pcie_aspm_check_latency()`
- `drivers/nvme/host/pci.c` — `nvme_configure_apst()`, APST state table
- `drivers/ata/libata-scsi.c` — ALPM policy
- `drivers/usb/core/driver.c` — USB modalias, `usb_match_id()`
- `drivers/thunderbolt/domain.c` — thunderbolt udev events

#### Documentation
- `Documentation/admin-guide/devices.rst`
- `Documentation/PCI/pci.rst`
- `Documentation/admin-guide/i915.rst` — DRM connector properties
- `Documentation/admin-guide/udev.rst`

#### Prior art (other projects with similar problems)
- `libinput` quirks DB — `/usr/share/libinput/*.quirks`
- `systemd-hwdb` — `systemd-hwdb(8)`
- `fwupd` device database — `https://github.com/fwupd/fwupd`
- `powertop --auto-tune` (anti-prior-art: no revert path)
- `tlp` runtime PM (`RUNTIME_PERPM`) — `https://linrunner.de/tlp/`
- `tuned` profiles — `https://tuned-project.org/`

#### Project-internal
- SPEC §3 (actuation rule), §4.3 (depth-enablers), §6 WP-N4 — `docs/SPEC-northstar.md`
- ADR-0009 — `docs/decisions/0009-optid-security-boundary.md`
- ADR-0013 — `docs/decisions/0013-detection-and-ml-boundary.md`
- Research 0002 — `docs/research/0002-rush-linux-architecture-review.md`
- Research 0003 — `docs/research/0003-unified-power-orchestrator-paper.md`

---

