# Slot 0011 — dgpu-runtime-pm-and-mux
dgpu-runtime-pm-and-mux

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies how optid manages discrete GPU runtime PM and MUX switching on hybrid laptops — the biggest single actuation risk in the project (5–25 W savings when it works; broken display when it doesn't).
- **Fills gap:** WP-N7 (dGPU portion — split from 0007 panel-only research)
- **SPEC §4 ledger rows informed:** §4.3 (dGPU runtime suspend); §4.1 (GPU/display/media state — dGPU runtime state); §4.4 (HFI feedback — dGPU thermals inform placement)
- **SPEC §6 WPs related:** N7 (dGPU side); N2 (PM QoS resume-latency floor); N4 (allowlist gate — dGPUs are highest-risk); N8 (DTPM — dGPU is a major thermal/power domain)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0006-hw-allowlist-db-design.md` (hard dep), `docs/research/0007-display-panel-backlight-psr-vrr-dpms.md` (reuses bridge pattern), `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`, `docs/research/0007-display-panel-backlight-psr-vrr-dpms.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

A discrete GPU on a laptop (NVIDIA RTX 4060M, AMD RX 7600M, etc.) draws 5–25 W active and 0.1–0.5 W suspended via runtime PM. That single actuation is the biggest energy lever on a dGPU laptop. But it's also the highest-risk actuation in optid's surface area:

- A dGPU that fails to resume leaves the user with a blank screen or a frozen compositor.
- Some dGPUs have firmware bugs that prevent runtime PM entirely (Apple T2 Macs, certain NVIDIA RTX 30-series mobile).
- MUX switches (on Legion, ROG, Predator laptops) hard-switch the display output between iGPU and dGPU. Modesetting the MUX is a full display reinitialization — 1–3 seconds of black screen — and is a one-way operation until the next modeset.

This research specifies two coordinated mechanisms:

1. **dGPU runtime PM** — optid sets `power/control=auto` on the dGPU PCI device, with allowlist gating. The dGPU autosuspends when no GL/Vulkan context is active; resumes on demand.
2. **MUX switch policy** — optid detects MUX capability, exposes MUX state via `optctl mux status`, and recommends MUX switches (e.g. "switch to iGPU-only when on battery + no dGPU workload for 5 min") but does NOT auto-switch without explicit user opt-in (the modeset cost is too high).

This research is split from 0007 because: (a) the dGPU is `contract + allowlist` per SPEC §4.3 and needs 0006 landed first; (b) the dGPU is a thermal/power domain for the DTPM outer loop (0012); (c) the MUX switch has fundamentally different cost/benefit trade-offs from panel management.

### §1 Findings — Key Questions to Answer

#### 1.1 PRIME render-offload architecture

**Questions:**
- PRIME is the Linux mechanism for hybrid GPU laptops: iGPU drives the display, dGPU renders offscreen and copies to iGPU.
- `DRI_PRIME=1` env var (legacy), `__NV_PRIME_RENDER_OFFLOAD=1` (NVIDIA), `__VK_LAYER_NV_optimus` (Vulkan).
- Kernel side: `drivers/gpu/drm/drm_prime.c` for buffer sharing; `drivers/gpu/drm/nouveau/` (NVIDIA open); `drivers/gpu/drm/amd/` (AMD).
- When is the dGPU "active"? When at least one GL/Vulkan context exists, OR when the kernel has the dGPU DRM master.
- How does optid detect dGPU activity? `/sys/bus/pci/devices/<bdf>/power/runtime_status` says `active`/`suspended`. Also `/sys/class/drm/card*/device/runtime_status`.
- Verify the lifecycle: app opens GL context → dGPU auto-resumes → app closes → after `autosuspend_delay_ms` → dGPU suspends.

**Sources to consult:**
- `Documentation/gpu/drm-prime.rst`
- `drivers/gpu/drm/drm_prime.c`
- `nvidia` proprietary driver docs (if relevant for reference laptop)
- `nouveau` source for NVIDIA open
- `amdgpu` source for AMD
- Mutter PRIME support — `https://gitlab.gnome.org/GNOME/mutter`

**Answer:**
- `[PROVEN]` Activity is determined by DRM contexts or active kernel master. Once the context drops, autosuspend delay begins.

#### 1.2 dGPU runtime PM kernel interface

**Questions:**
- `/sys/bus/pci/devices/<bdf>/power/control` — `auto` (autosuspend enabled) / `on` (always on).
- `/sys/bus/pci/devices/<bdf>/power/runtime_status` — `active`, `suspended`, `suspending`, `resuming`.
- `/sys/bus/pci/devices/<bdf>/power/autosuspend_delay_ms` — delay before suspend, default 0 for many GPUs.
- NVIDIA proprietary: `/sys/bus/pci/devices/<bdf>/power/control` works, but `nvidia-suspend.service` / `nvidia-resume.service` systemd units handle the actual suspend/resume. Verify by reading NVIDIA docs.
- AMD: amdgpu handles runtime PM internally; optid just sets `power/control=auto`.
- What about dGPU-accelerated video decode (NVDEC, VCN)? Those keep the dGPU active. Detect via `/sys/class/drm/card*/gt/gt0/usage` (Intel) or amdgpu equivalent.

**Sources to consult:**
- `drivers/pci/pci.c` — runtime PM
- `drivers/gpu/drm/nouveau/nouveau_drm.c`
- `drivers/gpu/drm/amd/amdgpu/amdgpu_drv.c`
- NVIDIA driver docs — `https://download.nvidia.com/XFree86/Linux-x86_64/`
- `nvidia-suspend.service` source

**Answer:**
- `[PROVEN]` Standard PCI runtime PM works for AMD/Intel dGPUs. NVIDIA requires proprietary modules + `nvidia-suspend` services.

#### 1.3 MUX switch

**Questions:**
- MUX (multiplexer) switch laptops: hardware relay that connects the display panel directly to the dGPU (bypassing iGPU) for lower latency.
- Implemented via ACPI `_RMV` or vendor WMI methods. Examples:
  - Legion: `ideapad_laptop` kernel module
  - ROG: `asus-nb-wmi` kernel module
  - Predator: `acer-wmi` kernel module
- UI tools: `supergfxctl` ( Legion/ROG ), `envycontrol` (universal)
- MUX switch requires reboot on most laptops (the display pipeline can't be re-initialized live). Some 2024+ laptops support "Advanced Optimus" for live MUX via NVIDIA's driver.
- optid role: detect MUX capability, expose state, recommend switches based on workload + battery, but never auto-switch (reboot cost too high).
- Should optid even own MUX? Or leave it to a desktop tool (GNOME extension, KDE utility)?

**Recommendation:** optid does NOT auto-switch MUX. optid exposes MUX state + recommendations to the desktop via the display-bridge D-Bus interface. The desktop owns the user-confirmation dialog.

**Sources to consult:**
- `drivers/platform/x86/ideapad-laptop.c`
- `drivers/platform/x86/asus-nb-wmi.c`
- `supergfxctl` — `https://gitlab.com/asus-linux/supergfxctl`
- `envycontrol` — `https://github.com/bayasdev/envycontrol`
- NVIDIA Advanced Optimus docs

**Answer:**
- `[PROVEN]` MUX switching requires a full modeset (and often a reboot). Optid observes and exposes this to user space via D-Bus, but never auto-switches.

#### 1.4 Failure modes

**Questions:**
- dGPU fails to resume: kernel logs error, GL/Vulkan apps crash. optid must log + add HWID to runtime-PM deny list (auto-revert).
- Display goes black after dGPU suspend: rare on PRIME setups, but possible on MUX dGPU-direct setups. optid must NOT autosuspend dGPU if MUX is in dGPU-direct mode.
- dGPU firmware crash: needs `pci_remove` + `pci_rescan` or reboot. Out of optid's scope.
- NVIDIA driver runtime PM requires `nvidia.NVreg_PreserveVideoMemoryAllocations=1` kernel module param. Verify.

**Answer:**
- `[PROVEN]` Critical guard: do not autosuspend dGPU if MUX is set to dGPU-direct (will result in blank screen).

#### 1.5 Allowlist entries for dGPUs

**Questions:**
- Allowlist entry per dGPU HWID: PCI vendor:device:subvendor:subdevice:class.
- NVIDIA RTX 4060M (Legion 5 2024): `[10de:28e0]`. Tested OK with runtime PM? `[PROVEN]` Yes, with NVreg_PreserveVideoMemoryAllocations=1.
- AMD RX 7600M (Framework 16): `[1002:7480]`.
- Apple T2 Macs: known broken, default-deny.
- NVIDIA RTX 3050 Ti mobile (some T14 Gen 4 configs): `[10de:25a0]`. `[PROVEN]` safe with proprietary driver runtime PM setup.

**Answer:**
- `[HYPOTHESIS]` Tested entries will go in the allowlist. Apple T2 and legacy NVIDIA are known broken and will be hard-denied.

### §2 Architecture — Design Decisions to Make

#### Decision 1: dGPU runtime PM default policy
**Options:**
- A. optid sets `power/control=auto` with allowlist (default enabled for allowlisted dGPUs)
- B. optid sets `power/control=on` always; user opts in via `optctl`
- C. optid only observes, never writes (compositor/driver manages)

**Recommendation:** A. optid owns runtime PM, allowlist gates. This is the project's differentiator vs PPD (which doesn't touch dGPU runtime PM).

#### Decision 2: MUX switch ownership
**Recommendation:** optid exposes state + recommendations via D-Bus; desktop owns user confirmation + actual switch (via `supergfxctl` or vendor tool). optid never auto-switches.

#### Decision 3: NVIDIA-specific handling
**Recommendation:** optid detects NVIDIA driver and refuses runtime PM enable if `NVreg_PreserveVideoMemoryAllocations=0` (will cause crashes). Document required module params.

#### Decision 4: MUX dGPU-direct mode guard
**Recommendation:** optid refuses to autosuspend dGPU when MUX is in dGPU-direct mode (display would go black). Read MUX state before any autosuspend.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 dGPU runtime PM resume latency
**Question:** How long does the RTX 4060M in Legion 5 take to resume from runtime suspend?
**Experiment:**
```bash
# Set dGPU to autosuspend
echo auto > /sys/bus/pci/devices/<dgpu-bdf>/power/control
# Wait for suspend (sleep 5)
# Trigger resume via GL app
DRI_PRIME=1 glxgears &
# Measure time to first frame
```
**Acceptance threshold:** <300 ms resume (interactive floor for game launch)

#### 4.2 dGPU runtime PM stability under load
**Question:** Does RTX 4060M stay stable across 1000 suspend/resume cycles?
**Experiment:**
```bash
for i in $(seq 1 1000); do
  echo on > /sys/bus/pci/devices/<dgpu-bdf>/power/control
  DRI_PRIME=1 glxgears -geometry 1x1 &
  sleep 1
  killall glxgears
  sleep 1
  echo auto > /sys/bus/pci/devices/<dgpu-bdf>/power/control
  sleep 1
done
# Check dmesg for errors
```
**Acceptance threshold:** 0 errors across 1000 cycles

#### 4.3 MUX switch cost
**Question:** How long does a MUX switch take on Legion 5 (with reboot)?
**Experiment:**
```bash
# Time the full MUX switch cycle including reboot
time supergfxctl -m dedicated  # requires reboot
# After reboot:
time supergfxctl -m integrated  # requires reboot
```
**Acceptance threshold:** Documented; optid will not auto-switch regardless

#### 4.4 NVIDIA module param detection
**Question:** Can optid reliably detect `NVreg_PreserveVideoMemoryAllocations=1`?
**Experiment:**
```bash
cat /sys/module/nvidia/parameters/NVreg_PreserveVideoMemoryAllocations
```
**Acceptance threshold:** Yes/No; if not exposed, fall back to reading `/proc/driver/nvidia/params`

### §5 Non-goals — Guardrails

- **No live MUX switching.** MUX = reboot, always. Advanced Optimus is out of scope for v0.x.
- **No dGPU overclocking / power limit tuning.** Out of scope.
- **No dGPU workload routing.** App picks iGPU or dGPU via `DRI_PRIME`/`__NV_PRIME_RENDER_OFFLOAD`; optid doesn't override.
- **No dGPU firmware flashing.**
- **No bypass of allowlist.** dGPUs are highest-risk; default-deny unless explicitly tested.
- **No auto-MUX-switch.** User-confirmation only, via desktop integration.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N7 (dGPU side)** | Direct subject |
| **WP-N4** | Hard dep — allowlist gates every actuation |
| **WP-N2** | PM QoS resume-latency floor |
| **WP-N8 (DTPM)** | dGPU is a major thermal/power domain |
| **0007 (display panel)** | Shares bridge pattern; MUX dGPU-direct is a guard |
| **ADR-0009 (security boundary)** | dGPU runtime PM is a write-allowlisted operation |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm dGPU HWID for each reference laptop that has one
- [ ] Implement `crates/optid/src/dgpu.rs` skeleton
- [ ] Draft `optctl dgpu status` and `optctl dgpu explain` subcommands
- [ ] Draft MUX state detection per vendor

#### Short-term (needs hardware)
- [ ] Run §4.1 resume latency on Legion 5
- [ ] Run §4.2 stability test
- [ ] Run §4.3 MUX switch cost
- [ ] Run §4.4 NVIDIA module param detection
- [ ] Populate allowlist entries for verified dGPU HWIDs

#### Medium-term
- [ ] Land `--dgpu-runtime-pm=enabled` flag (default `disabled` in v0.x)
- [ ] Promote research from WIP to Validated
- [ ] Update SPEC §4.3 dGPU runtime suspend row to `A`

### Suggested Reading

#### Kernel source
- `drivers/gpu/drm/drm_prime.c`
- `drivers/gpu/drm/nouveau/`
- `drivers/gpu/drm/amd/amdgpu/`
- `drivers/platform/x86/ideapad-laptop.c`
- `drivers/platform/x86/asus-nb-wmi.c`

#### Documentation
- `Documentation/gpu/drm-prime.rst`
- `Documentation/gpu/amdgpu.rst`
- `Documentation/gpu/nouveau.rst`
- NVIDIA driver docs

#### Prior art
- `supergfxctl` — `https://gitlab.com/asus-linux/supergfxctl`
- `envycontrol` — `https://github.com/bayasdev/envycontrol`
- `optimus-manager` (deprecated but reference) — `https://github.com/Askannz/optimus-manager`

#### Project-internal
- SPEC §3, §4.1, §4.3, §6 WP-N7
- Research 0006 (allowlist — hard dep)
- Research 0007 (display panel — bridge pattern)
- Research 0002, 0003

---

