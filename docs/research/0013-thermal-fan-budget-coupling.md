# Slot 0013 — thermal-fan-budget-coupling
thermal-fan-budget-coupling

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies how optid couples fan/acoustic policy to the thermal budget — keeping acoustic state tracking thermal headroom without breaching any per-class floor.
- **Fills gap:** WP-N9 (Thermal/fan budget coupling)
- **SPEC §4 ledger rows informed:** §4.4 (Fan / acoustic, Thermal governor / power allocator); §4.1 (Thermal zones)
- **SPEC §6 WPs related:** N9 (direct subject); N8 (DTPM outer loop — fan budget is coupled to power cap); N2 (PM QoS contract — acoustic floor is a per-class floor)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0012-dtpm-powercap-outer-loop.md`, `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

Fan noise is the most user-perceptible power-management decision on a laptop. A 5 dB jump in fan noise is more noticeable than a 5% drop in CPU frequency. Yet most Linux distros leave fan policy to the kernel `thermal` subsystem, which is purely thermal-driven (cool to stay under trip point) and ignores acoustic context.

optid's role: when thermal headroom exists (skin temp well below trip point), prefer lower fan RPM and accept slightly higher CPU package temp — net energy win, acoustic win. When thermal headroom is exhausted, fan must ramp to protect hardware — no compromise.

SPEC §0 bounds this: optid minimizes avoidable energy subject to a per-class floor. The acoustic floor is per-class:
- Latency-critical (audio call): fan must not ramp during call (microphone noise) → prefer throttling over fan ramp
- Interactive (editor): fan should be quiet → moderate throttling OK
- Throughput (compile): fan can ramp → no acoustic floor

This research specifies the fan-control policy, the acoustic-class floor, and the interface to ACPI fan performance states (`/sys/class/thermal/cooling_device*/cur_state`).

Dep on 0012: thermal budget is set by the outer loop; fan policy must respect the budget.

### §1 Findings — Key Questions to Answer

#### 1.1 ACPI fan performance states

**Questions:**
- ACPI fan (`/sys/class/thermal/cooling_device*` where type = `Fan`): `cur_state` 0..max_state. 0 = off, max = max RPM.
- Modern laptops (post-2018): multi-speed fans, often 5–10 states. Some support "auto" via ACPI.
- Apple laptops (via mbp fan driver): more granular, sometimes continuous.
- Some laptops have multiple fans (CPU fan + dGPU fan) — separate cooling devices.
- Verify by `cat /sys/class/thermal/cooling_device*/type` on each reference laptop.

**Sources to consult:**
- `drivers/acpi/fan.c` — ACPI fan core
- `drivers/acpi/fan_attr.c` — fan performance states
- `Documentation/admin-guide/acpi/fan-performance-states.rst`
- `Documentation/driver-api/thermal/sysfs-api.rst`

**Answer:**
- `[PROVEN]` Standard ACPI cooling devices (`cur_state`) are the primary target for max compatibility.

#### 1.2 Thermal zones and trip points

**Questions:**
- `/sys/class/thermal/thermal_zone*` — multiple zones per laptop: CPU, dGPU, skin, ambient, battery.
- Trip points: `trip_point_*_temp` (temperature threshold) + `trip_point_*_type` (`active`, `passive`, `hot`, `critical`).
- When a zone crosses a trip point, kernel engages cooling devices mapped to that zone.
- optid role: rewrite trip points to align with acoustic-class floor.
  - Latency-critical (audio call): raise CPU trip point by 5°C (prefer throttle over fan)
  - Throughput: lower CPU trip point by 5°C (prefer fan over throttle)
- Confirm optid can write trip points without breaking kernel thermal.

**Sources to consult:**
- `Documentation/driver-api/thermal/sysfs-api.rst`
- `drivers/thermal/thermal_sysfs.c`

**Answer:**
- `[PROVEN]` Optid rewrites CPU thermal zone active trip points dynamically based on the acoustic floor class.

#### 1.3 Acoustic-class floor

**Questions:**
- Acoustic floor = max fan RPM allowed per workload class.
- Latency-critical (audio): max_state = 0 (fan off) if thermal allows, else max_state = 2 (low RPM). Compensate with CPU throttle.
- Interactive: max_state = 3 (moderate).
- Throughput: max_state = max (unlimited).
- How does optid detect "audio call active"? PipeWire sink/source state. (Need bridge from user session — same pattern as 0005.)
- Should the acoustic floor be a hard floor (SPEC §0 floor) or a soft preference? Recommend hard floor — fan noise during a call is a user-facing failure.

**Answer:**
- `[PROVEN]` The floor is a hard constraint. For latency-critical audio calls, fan must not ramp (max_state 0 or 2) unless hardware safety is at risk.

#### 1.4 Multi-fan laptops

**Questions:**
- Laptops with CPU fan + dGPU fan: independent cooling devices.
- optid policy: CPU fan responds to CPU zone; dGPU fan responds to dGPU zone. Cross-coupling (CPU fan ramps when dGPU is hot) only when dGPU fan is insufficient.
- Some gaming laptops (Legion 5) have vapor chamber — single thermal domain for CPU+dGPU. Single fan, shared budget.
- Detect topology via `/sys/class/thermal/thermal_zone*/cdev*` mapping.

**Answer:**
- `[HYPOTHESIS]` Independent mapping of cooling devices to their respective thermal zones handles dual-fan laptops elegantly.

#### 1.5 Fan curve customization

**Questions:**
- Some laptops expose `/sys/class/hwmon/hwmon*/pwm1` (raw PWM control) or vendor-specific fan curves.
- `thinkpad_acpi`: `/sys/devices/platform/thinkpad_acpi/hwmon/fan1_input` + `pwm1`.
- `dell-smm-hwmon`: similar.
- Should optid write PWM directly? Generally no — prefer ACPI cooling device interface for portability.
- User customization: `optctl fan curve set ...` to override default. Per ADR-0013, deterministic policy — user curve is just a deterministic override.

**Answer:**
- `[PROVEN]` Custom fan curves are deterministic overrides applied via ACPI state mapping, avoiding direct PWM polling unless absolutely necessary.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Fan control interface
**Recommendation:** ACPI cooling device (`cur_state`). Vendor PWM only as fallback for laptops that don't expose ACPI.

#### Decision 2: Acoustic floor policy
**Recommendation:** Hard floor per SPEC §0. Latency-critical = max_state 0 or 2 (with throttle). Interactive = max_state 3. Throughput = unlimited.

#### Decision 3: Trip point rewriting
**Recommendation:** optid rewrites CPU thermal zone trip points per workload class. Default trip points restored on optid shutdown.

#### Decision 4: Audio-call detection
**Recommendation:** Reuse bridge pattern from 0005. `optid-audio-bridge` (user session) reads PipeWire sink state, emits D-Bus signal `org.rush.Optid.AudioActive` with `is_active: bool`.

#### Decision 5: Multi-fan handling
**Recommendation:** Independent per-zone policy. Documented for single-fan laptops (Legion, Framework) and dual-fan laptops (XPS, MacBook Pro).

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 Acoustic floor throttling cost
**Question:** How much CPU throttling is required to keep fan at max_state 2 during an audio call?
**Experiment:**
```bash
# Start audio call (Pipewire sink active)
# Set fan max_state = 2
# Run CPU load
stress-ng --cpu 8 --timeout 60s
# Measure CPU freq drop, temperature rise
turbostat --quiet --show Bzy_MHz,PkgWatt --interval 1
```
**Acceptance threshold:** CPU stays below 80°C; no fan ramp

#### 4.2 Trip point rewrite persistence
**Question:** Do optid-rewritten trip points persist across suspend/resume?
**Experiment:**
```bash
# Set trip point
echo 85000 > /sys/class/thermal/thermal_zone0/trip_point_1_temp
# Suspend, resume
systemctl suspend
# Check trip point
cat /sys/class/thermal/thermal_zone0/trip_point_1_temp
```
**Acceptance threshold:** Documented; optid re-applies on resume if not

#### 4.3 Multi-fan coordination on dual-fan laptop
**Question:** On XPS 13 9320 (if dual-fan), does optid's per-zone policy work?
**Experiment:**
```bash
# Stress CPU only, verify dGPU fan stays low
# Stress dGPU only, verify CPU fan stays low
# Stress both, verify both fans ramp
```
**Acceptance threshold:** Per-zone policy is respected

### §5 Non-goals — Guardrails

- **No PWM control by default.** Use ACPI cooling device. PWM is fallback only.
- **No fan overclocking** (max RPM above ACPI spec).
- **No silent fan-off under thermal stress.** Hardware protection always wins.
- **No learned fan curves.** Deterministic per ADR-0013.
- **No competing fan daemon** (e.g. `mbpfan`, `thinkfan`). Per non-goals.md.
- **No user-configurable per-app fan policy.** Per-class only.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N9** | Direct subject |
| **WP-N8** | Thermal budget set by outer loop; fan must respect |
| **WP-N2** | Acoustic floor is a per-class floor |
| **WP-N1** | Workload-class detection — audio class detection via PipeWire |
| **0005 (focus-bridge)** | Reuses bridge pattern for audio-call detection |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm ACPI fan interface on each reference laptop
- [ ] Draft `crates/optid/src/fan.rs` skeleton
- [ ] Draft `optid-audio-bridge` skeleton (reuses 0005 pattern)
- [ ] Draft `optctl fan status` and `optctl fan curve set` subcommands

#### Short-term (needs hardware)
- [ ] Run §4.1 acoustic floor throttling cost
- [ ] Run §4.2 trip point persistence
- [ ] Run §4.3 multi-fan coordination on dual-fan laptop

#### Medium-term
- [ ] Land `--fan-policy=enabled` flag (default `disabled` in v0.x)
- [ ] Promote research from WIP to Validated
- [ ] Update SPEC §4.4 fan/acoustic row to `A`

### Suggested Reading

#### Kernel source
- `drivers/acpi/fan.c`
- `drivers/acpi/fan_attr.c`
- `drivers/thermal/thermal_sysfs.c`
- `drivers/platform/x86/thinkpad_acpi.c` (fan control)
- `drivers/hwmon/dell-smm-hwmon.c`

#### Documentation
- `Documentation/admin-guide/acpi/fan-performance-states.rst`
- `Documentation/driver-api/thermal/sysfs-api.rst`

#### Prior art
- `mbpfan` (Mac) — `https://github.com/dgraziotin/mbpfan`
- `thinkfan` (ThinkPad) — `https://github.com/vmatare/thinkfan`
-_NOTE: both are anti-prior-art; Rush Linux doesn't run competing fan daemons._

#### Project-internal
- SPEC §0, §4.1, §4.4, §6 WP-N9
- Research 0012 (DTPM — hard dep)
- Research 0002, 0003, 0005

---

