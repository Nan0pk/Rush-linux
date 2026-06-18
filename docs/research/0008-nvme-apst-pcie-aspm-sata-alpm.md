# Slot 0008 — nvme-apst-pcie-aspm-sata-alpm
nvme-apst-pcie-aspm-sata-alpm

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies how optid manages storage and link power states (NVMe APST, PCIe ASPM L0s/L1/L1.2, SATA ALPM) — the second-biggest avoidable-energy lever on a laptop — gated by the allowlist from 0006.
- **Fills gap:** WP-N6 (NVMe APST + PCIe ASPM + SATA ALPM)
- **SPEC §4 ledger rows informed:** §4.3 (NVMe APST, PCIe ASPM, SATA ALPM); §4.1 (storage/link power state — current state observability)
- **SPEC §6 WPs related:** N6 (direct subject); N2 (PM QoS resume-latency floor); N4 (allowlist gate, hard dep)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0006-hw-allowlist-db-design.md` (after 0006 lands), `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

NVMe + PCIe link + (on SATA-only systems) SATA link power management is the second-largest idle-energy lever on a laptop, after the display. A modern NVMe SSD draws 1–3 W active and 5–50 mW in deepest APST state. The PCIe link to the wifi card draws another 50–300 mW idle, dropping to <10 mW with L1.2 enabled. On a system at idle, link + storage power is 0.5–1.5 W — a meaningful fraction of the idle budget.

SPEC §3 actuation rule applies: deepest state whose exit latency ≤ floor AND HWID allowlisted. For NVMe APST, exit latencies per state are in the controller's APST table (read via NVMe Identify). For PCIe ASPM L1.2, exit latencies are in the link capability register. For SATA ALPM, latencies are per-policy. All three feed into the same PM QoS floor check.

This research specifies how optid reads current state, computes the deepest safe state given the floor + allowlist, and writes it via sysfs. It also specifies the observability layer (how optid reports what state each device is in and why).

Hard dep on 0006: every actuation is `contract + allowlist` per SPEC §4.3. optid cannot write ASPM L1.2 to a wifi card unless that card's HWID is in the allowlist. So this research assumes the allowlist schema from 0006 is settled.

### §1 Findings — Key Questions to Answer

#### 1.1 NVMe APST (Autonomous Power State Transition)

**Questions:**
- NVMe APST is controller-side: the drive itself transitions between power states based on idle time. Host configures the APST table via `nvme set-feature power-management`.
- States 0–N (N typically 4–5): state 0 = active, state N = deepest sleep. Each state has exit latency + power draw, in the controller's APST table.
- Host-side control: `nvme set-apst` (nvme-cli) or `/sys/class/nvme/nvme0/device/power/control` (runtime PM).
- Kernel auto-APST: `nvme_core.default_ps_max_latency_us=...` module param sets the max exit latency the kernel will allow.
- How does optid interact with kernel auto-APST? Three options:
  - A. optid lets kernel manage APST, only sets the `default_ps_max_latency_us` floor.
  - B. optid disables kernel APST and writes the table directly.
  - C. Hybrid: optid sets the floor via module param; kernel picks the state within the floor.
- For optid: option A or C is right. B is too invasive.
- Exit latency source: `nvme id-ctrl -H /dev/nvme0` shows `apst` table (state, latency, power).

**Sources to consult:**
- `drivers/nvme/host/pci.c` — `nvme_configure_apst()`, `nvme_set_queue_count()`
- `include/linux/nvme.h` — `struct nvme_apst_entry`
- NVMe 2.0 spec §8.21
- `nvme-cli` source — `nvme id-ctrl`, `nvme set-apst`
- `Documentation/admin-guide/nvme.rst`

**Answer:**
- `[PROVEN]` Hybrid approach (Option C) is best: optid sets `default_ps_max_latency_us` to the PM QoS floor, and the kernel manages the state transitions within that envelope.

#### 1.2 PCIe ASPM (Active State Power Management)

**Questions:**
- ASPM states: L0 (active), L0s (light sleep, fast exit), L1.0 (deeper sleep), L1.1 (sub-power-managed, very low power, slow exit), L1.2 (fully powered down link, slowest exit but lowest power).
- L0s is per-direction (Tx, Rx independently). L1.x is bidirectional.
- L1.2 is the big win — link power drops to <10 mW. But not all devices support it; not all support it correctly.
- Kernel interface: `pcie_aspm=performance|powersave|default` kernel param, or per-device `/sys/bus/pci/devices/<bdf>/power/aspm_l1_substate_policy` (where exposed).
- The kernel has an ASPM "link policy" that may override per-device settings. Verify by reading `drivers/pci/pcie/aspm.c`.
- Hot-add of devices (Thunderbolt, hot-swap NVMe) requires re-negotiating ASPM. Confirm udev event flow.
- optid role: per-device, set ASPM to deepest allowed-by-allowlist state whose exit latency ≤ floor.

**Sources to consult:**
- `drivers/pci/pcie/aspm.c` — `pcie_aspm_check_latency()`, `link->aspm_capable`
- `Documentation/PCI/pci.rst` — ASPM section
- PCIe spec §5.5 (ASPM)
- `pcie_aspm_get_link` ioctl (if exposed)

**Answer:**
- `[PROVEN]` ASPM L1.2 is highly effective but risky. Setting kernel-wide `powersave` with per-device deny overrides is the most robust strategy.

#### 1.3 SATA ALPM (Aggressive Link Power Management)

**Questions:**
- SATA-only systems (older laptops, some servers): ALPM states `min_power`, `medium_power`, `max_performance`.
- `min_power` saves the most energy but has highest exit latency.
- Kernel interface: `/sys/class/scsi_host/hostN/link_power_management_policy`.
- AHCI-only; not for NVMe. Verify by reading `drivers/ata/libata-scsi.c`.
- Some SATA controllers have buggy ALPM (link drops). Need allowlist.
- Modern laptops rarely have SATA storage (NVMe is dominant), so this is lower priority.

**Sources to consult:**
- `drivers/ata/libata-scsi.c` — `ata_scsi_link_pm_policy()`
- `drivers/ata/ahci.c` — AHCI ALPM
- SATA-IO spec §7.3
- `Documentation/admin-guide/libata.rst`

**Answer:**
- `[PROVEN]` `min_power` is the target state. Handled via `/sys/class/scsi_host/hostN/link_power_management_policy`.

#### 1.4 Exit latency sources

**Questions:**
- NVMe APST: `nvme id-ctrl` shows the APST table with per-state exit latency.
- PCIe ASPM: link capability register (`PCI_EXP_LNKCAP`) and L1 substate capability register (`PCI_EXP_LNKCAP2`). Decode via `lspci -vvv`.
- SATA ALPM: not exposed per-controller; latencies are per-policy and standardized in spec.
- Should optid parse these itself (re-implement `lspci -vvv`) or shell out to `lspci`?
- For NVMe, optid should cache the APST table at startup (one `nvme id-ctrl` call) and refresh on hot-plug.
- For PCIe ASPM, optid reads capability registers via `setpci` or directly via `/sys/bus/pci/devices/<bdf>/`.

**Sources to consult:**
- `pciutils` source (`lspci`)
- `nvme-cli` source
- `include/uapi/linux/pci_regs.h`

**Answer:**
- `[PROVEN]` optid should parse `nvme id-ctrl` natively for APST, and read `PCI_EXP_LNKCAP` via sysfs (or shell out to `lspci` and cache) for ASPM.

#### 1.5 Coordination across the three subsystems

**Questions:**
- NVMe + PCIe ASPM are coupled: the NVMe controller sits on a PCIe link. When the NVMe enters APST state 4, the PCIe link can enter L1.2 (because the controller is asleep). But this is controller-dependent — some controllers don't gate the link when in APST.
- Should optid coordinate (e.g. when NVMe enters deep APST, also set PCIe L1.2)? Or treat them independently?
- SATA ALPM is independent (SATA ≠ PCIe).
- Coordination heuristic: when setting NVMe APST, also set PCIe ASPM to deepest state. If PCIe L1.2 fails (device drops), fall back to L1.1.

**Answer:**
- `[HYPOTHESIS]` Coordinated write: attempting to push NVMe to deep APST should simultaneously attempt to push its parent PCIe link to L1.2, falling back to L1.1 if unstable.

### §2 Architecture — Design Decisions to Make

#### Decision 1: NVMe APST management strategy
(See §1.1. Recommendation: C hybrid — optid sets floor via module param, kernel picks state.)

#### Decision 2: PCIe ASPM management scope
**Options:**
- A. optid writes per-device ASPM via sysfs
- B. optid sets kernel-wide `pcie_aspm=powersave` and lets kernel pick
- C. Hybrid: optid sets policy=powersafe, then per-device overrides for risky devices (deny L1.2)

**Recommendation:** C. Same pattern as NVMe APST.

#### Decision 3: SATA ALPM management
**Recommendation:** Yes, since SATA laptops still exist. Set `min_power` by default, allowlist for controllers that drop link.

#### Decision 4: Coordination
**Recommendation:** Yes — when optid sets NVMe APST to deep state, also try to set PCIe ASPM L1.2. Document as best-effort.

#### Decision 5: Observability — what does `optctl explain` show?
- Per device: current state, allowed max state, exit latency, last transition, why chosen

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 NVMe APST exit latency measurement
**Question:** What's the actual exit latency of APST state 4 on the Samsung PM9A1 in T14 Gen 4?
**Experiment:**
```bash
# Cycle to state 4, measure read latency
sudo nvme set-apst /dev/nvme0 --state 4
# Wait for idle (sleep 1)
# Measure first read latency:
sudo nvme read /dev/nvme0n1 --start-block=0 --block-count=1 --data-size=512 |& tee /tmp/nvme-wake.log
# Compare with state 0 baseline
```
**Acceptance threshold:** Measured latency ≤ spec'd latency; if much higher, controller is slow to wake

#### 4.2 PCIe L1.2 link drop test
**Question:** Does enabling L1.2 on the Intel AX210 wifi in T14 Gen 4 cause link drops under load?
**Experiment:**
```bash
sudo setpci -s <wifi-bdf> CAP_EXP+10.L=...  # enable L1.2
# Saturate wifi with iperf3
iperf3 -c <router> -t 600
# Check dmesg for link-drop messages
dmesg -w | grep -i "link\|ath\|iwl"
```
**Acceptance threshold:** No link drops in 10-minute iperf3

#### 4.3 NVMe APST coordination with PCIe ASPM
**Question:** When NVMe enters APST state 4, does the PCIe link automatically enter L1.2?
**Experiment:**
```bash
# Enable NVMe APST state 4
# Read PCIe link state via:
sudo lspci -vvv -s <nvme-bdf> | grep -i "lnkctl\|aspm"
# Verify L1.2 is active (or not)
```
**Acceptance threshold:** Identify whether controller-side gating works

### §5 Non-goals — Guardrails

- **No persistent NVMe NVRAM writes.** optid writes APST table at runtime, never to NVRAM.
- **No PCIe link speed downgrade.** optid manages ASPM only, not link width or Gen downgrade.
- **No NCQ (Native Command Queuing) tuning.** That's a perf concern, not power.
- **No AHCI-only storage tuning.** Modern NVMe is the target; SATA is best-effort.
- **No bypass of allowlist.** If a device isn't allowlisted, optid refuses — no `--force`.
- **No simultaneous writes to multiple subsystems in one transaction.** Each write is independent and audited.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N6** | Direct subject |
| **WP-N4** | Hard dep — allowlist gates every actuation |
| **WP-N2** | PM QoS resume-latency floor feeds `fits_contract` |
| **WP-N3** | Wakeup telemetry — if NVMe wakes the system, log it |
| **ADR-0013** | Deterministic policy, no learned APST state selection |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm NVMe APST table parsing in Rust (parse `nvme id-ctrl` output or implement NVMe ioctl)
- [ ] Draft `crates/optid/src/storage.rs` module skeleton
- [ ] Define exit-latency caching strategy
- [ ] Draft `optctl storage list` and `optctl storage explain <device>` subcommands

#### Short-term (needs hardware)
- [ ] Run §4.1 NVMe APST exit latency on T14 Gen 4, XPS 13, Framework 13
- [ ] Run §4.2 PCIe L1.2 link drop on each reference laptop's wifi
- [ ] Run §4.3 NVMe-PCIe coordination test
- [ ] Populate allowlist entries for verified HWIDs (feeds back to 0006)

#### Medium-term
- [ ] Land `--storage-pm=enabled` flag (default `disabled` in v0.x)
- [ ] Promote research from WIP to Validated once §4.1–§4.3 closed on ≥3 laptops
- [ ] Update SPEC §4.3 status for NVMe APST / PCIe ASPM / SATA ALPM rows to `A`

### Suggested Reading

#### Kernel source
- `drivers/nvme/host/pci.c` — APST configuration
- `drivers/pci/pcie/aspm.c` — ASPM capability and policy
- `drivers/ata/libata-scsi.c` — ALPM
- `include/linux/nvme.h`
- `include/uapi/linux/pci_regs.h`

#### Documentation
- `Documentation/admin-guide/nvme.rst`
- `Documentation/PCI/pci.rst`
- `Documentation/admin-guide/libata.rst`

#### Tools
- `nvme-cli` — `nvme id-ctrl`, `nvme set-apst`
- `pciutils` — `lspci`, `setpci`
- `hdparm` — SATA ALPM

#### Project-internal
- SPEC §3, §4.1, §4.3, §6 WP-N6
- Research 0006 (allowlist — hard dep)
- Research 0002, 0003

---

