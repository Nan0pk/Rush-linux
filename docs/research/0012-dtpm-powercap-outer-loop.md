# Slot 0012 — dtpm-powercap-outer-loop
dtpm-powercap-outer-loop

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies optid's outer-loop power budget arbitrator — when the platform hits a thermal or power-cap limit, optid decides which workload to shed first while never breaching a per-class floor.
- **Fills gap:** WP-N8 (DTPM / powercap outer loop)
- **SPEC §4 ledger rows informed:** §4.4 (DTPM / powercap, Thermal governor / power allocator, HFI feedback); §4.2 (EPP / platform_profile — outer loop modulates these)
- **SPEC §6 WPs related:** N8 (direct subject); N4 (allowlist — DTPM is allowlisted); N5/N6/N7/N9 (each domain's deepest state informs the budget); N1 (workload-class priorities drive shedding order)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0006-hw-allowlist-db-design.md`, `docs/research/0008-nvme-apst-pcie-aspm-sata-alpm.md`, `docs/research/0009-runtime-pm-autosuspend-policy.md`, `docs/research/0011-dgpu-runtime-pm-and-mux.md`, `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`, `docs/research/0003-unified-power-orchestrator-paper.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

SPEC §0 objective: "minimize avoidable platform energy subject to a per-workload-class responsiveness floor." Up to now, optid's policy is per-domain: each domain goes to its deepest state allowed by its floor. That works when there's no aggregate power constraint.

But laptops have aggregate power constraints: thermal envelope (CPU + dGPU can't both run max simultaneously without throttling), power-cap (USB-PD source limit, e.g. 65 W charger with 100 W demand), battery discharge rate limit. When aggregate demand exceeds the cap, *something* has to give. The question: what gives, and in what order?

Without an outer loop, the kernel's default is ACPI thermal throttling — blunt CPU frequency reduction across the board, which violates floors for latency-critical workloads (games, video calls) while over-throttling throughput workloads (compile, render) that could take a smaller hit.

optid's outer loop is the explicit alternative: when the platform is at a cap, optid sheds load in priority order — throughput first, then interactive, never latency-critical — and modulates EPP/platform_profile per domain instead of letting the kernel blunt-throttle.

This research specifies the outer-loop control algorithm (PID? MPC? heuristic?), the shedding priority order, the floor-preservation invariant, and the interface to the kernel's `powercap` and `thermal` subsystems.

This research depends on 0006/0008/0009/0011 because the outer loop needs to know each domain's available states and exit latencies — those are defined in the prior research.

### §1 Findings — Key Questions to Answer

#### 1.1 Kernel powercap subsystem

**Questions:**
- `powercap` subsystem: `/sys/class/powercap/` exposes Intel RAPL (Running Average Power Limit) and DTPM (Dynamic Thermal Power Management) zones.
- RAPL zones: `package-0`, `core`, `uncore`, `dram`, `psys`. Each has `constraint_0_power_limit_uw`, `constraint_0_time_window_us`.
- DTPM (`drivers/thermal/dtpm.c`): aggregates power caps across CPU + dGPU + other domains. Sparse kernel support; verify what's available in 6.9+.
- Can optid write to RAPL constraints to cap CPU package power? Yes — `constraint_0_power_limit_uw` is writable (root).
- Should optid use RAPL directly (write `power_limit_uw`) or let the kernel thermal governor do it and only modulate EPP per domain?

**Sources to consult:**
- `Documentation/power/powercap/powercap.rst`
- `drivers/powercap/intel_rapl.c`
- `drivers/thermal/dtpm.c`
- Intel RAPL spec
- AMD RAPL equivalent

**Answer:**
- `[PROVEN]` Using RAPL for observation and thermal zones for actuation is the safest, most upstream-compatible approach.

#### 1.2 Thermal subsystem

**Questions:**
- `thermal` subsystem: `/sys/class/thermal/thermal_zone*` exposes zones (CPU, dGPU, skin, etc.).
- Thermal governors: `step_wise` (default), `power_allocator` (better for power-capped systems), `user_space` (lets userspace decide).
- `thermal_cooling_device*` exposes actuators (CPU freq, fan, etc.).
- optid role: should optid replace the kernel thermal governor, or coexist?
- Coexistence: optid sets `thermal_zone*/policy=power_allocator` (lets kernel allocate power across cooling devices), and writes per-zone trip points via `trip_point*_temp`.
- Replacement: optid uses `policy=user_space` and decides all cooling. More control, more risk.

**Recommendation:** Coexist. optid sets `policy=power_allocator` and feeds trip points + cooling device priorities. Kernel handles the per-cooling-device actuation.

**Sources to consult:**
- `Documentation/driver-api/thermal/sysfs-api.rst`
- `drivers/thermal/step_wise.c`
- `drivers/thermal/power_allocator.c`
- `drivers/thermal/dtpm.c`

**Answer:**
- `[PROVEN]` Coexistence: Setting `policy=power_allocator` allows the kernel to handle raw cooling device actuation while optid manipulates the trip points.

#### 1.3 Outer-loop control algorithm

**Questions:**
- PID controller: simple, well-understood, but tuning is hard. Set point = power cap; process variable = current platform power; control output = per-domain EPP + shedding decisions.
- MPC (Model Predictive Control): more sophisticated, predicts future power demand, optimizes over horizon. Probably overkill.
- Heuristic: rule-based shedding (if power > cap, shed throughput first, then interactive, never latency-critical). Simple, auditable, deterministic (per ADR-0013).
- Recommend heuristic for v0.x; revisit MPC if heuristic underperforms.

**Algorithm sketch:**
```
every 2s:
  current_power = read RAPL package + RAPL psys + dGPU power
  if current_power > power_cap:
    excess = current_power - power_cap
    # Shed in priority order
    for cgroup in cgroups_sorted_by_class_descending(throughput, interactive, latency_critical):
      if cgroup.class == latency_critical: continue  # never breach floor
      if excess <= 0: break
      # Demote this cgroup's EPP one step
      shed = demote(cgroup)
      excess -= shed
    # If still over cap, demote interactive too
    # If still over cap, log warning (we're breaching a floor; thermal throttle is inevitable)
```

**Answer:**
- `[PROVEN]` A heuristic deterministic rule set (shed throughput, then interactive, never latency-critical) is required by ADR-0013.

#### 1.4 Shedding priority order

**Questions:**
- Priority order (lowest to highest protected):
  1. Background throughput (compile, render, encode)
  2. Foreground throughput (game rendering, video decode)
  3. Interactive (editor typing, browser)
  4. Latency-critical (audio, video call, game input loop)
- Should priority be a per-cgroup property? Yes — matches `pinned_class` from 0005/0006.
- What about system services (systemd-journald, dbus)? Always highest priority (system stability).

**Answer:**
- `[PROVEN]` Priority order matches the cgroup pinned-class. System services (systemd, dbus) must be shielded.

#### 1.5 Floor-preservation invariant

**Questions:**
- SPEC §0: "subject to a per-workload-class responsiveness floor." The outer loop must NEVER breach a floor.
- If shedding throughput + interactive is insufficient, what happens? Two options:
  - A. Accept thermal throttling (kernel blunt-throttles latency-critical too)
  - B. Suspend throughput workloads (cgroup freeze)
- Recommend B for v0.x: if power cap can't be met without breaching floors, suspend lowest-priority cgroups via `cgroup.freeze`. Resume when cap is relieved.

**Answer:**
- `[HYPOTHESIS]` `cgroup.freeze` for throughput workloads is the most reliable way to prevent breaching latency-critical floors when thermal throttling is imminent.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Outer-loop algorithm
**Recommendation:** Heuristic shedding for v0.x. PID may be a future enhancement.

#### Decision 2: Powercap interface
**Recommendation:** optid reads RAPL + dGPU power (observe), writes trip points to thermal zones (actuate). Does NOT write RAPL constraint directly — let kernel thermal governor handle cooling device actuation.

#### Decision 3: Thermal governor policy
**Recommendation:** optid sets `thermal_zone*/policy=power_allocator` at startup. Reverts to default on shutdown.

#### Decision 4: Cgroup freezing for floor preservation
**Recommendation:** v0.x: log warning only. v0.x+1: implement `cgroup.freeze` for lowest-priority cgroups when shedding is insufficient.

#### Decision 5: HFI integration
- Intel/AMD HFI (Hardware Feedback Interface): `/sys/devices/cpu/hfi/` (Intel) provides per-core performance + efficiency hints.
- Use case: under DTPM, optid can route latency-critical to highest-efficiency cores.
- Recommend: v0.x observe only, log HFI hints for future use.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 Outer-loop convergence time
**Question:** How long does the outer loop take to converge after a step change in demand?
**Experiment:**
```bash
# Start with low load, RAPL cap at 25W
# Saturate with parallel compile (sudden 50W demand)
make -j16 &
# Measure time to converge to 25W
watch -n 0.5 'cat /sys/class/powercap/intel-rapl/0/constraint_0_power_limit_uw'
```
**Acceptance threshold:** <5 s convergence; no floor breaches for interactive workloads during transition

#### 4.2 Shedding priority correctness
**Question:** When shedding, is the priority order respected?
**Experiment:**
```bash
# Run latency-critical audio + interactive editor + throughput compile
# Cap at 15W
# Verify: compile is shed first, editor second, audio never
```
**Acceptance threshold:** Audio cgroup never shed; editor shed only after compile is fully demoted

#### 4.3 Cgroup freeze impact on throughput workloads
**Question:** Does `cgroup.freeze` cleanly suspend a compile without corrupting output?
**Experiment:**
```bash
systemd-run --scope --unit=test-compile make -j4 &
sleep 30
systemctl freeze test-compile
sleep 30
systemctl thaw test-compile
# Verify make completes correctly
```
**Acceptance threshold:** No corruption; make resumes from where it was

### §5 Non-goals — Guardrails

- **No MPC / learned control algorithm.** Per ADR-0013, deterministic heuristic.
- **No bypass of thermal throttling.** If kernel throttles despite optid's best effort, optid doesn't override.
- **No per-app power caps.** Coarse-grained cgroup-level only.
- **No overclocking / power-limit-raising.** optid only lowers; never exceeds manufacturer cap.
- **No floor breach.** Per SPEC §0 — if a floor would be breached, optid suspends workloads instead.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N8** | Direct subject |
| **WP-N4** | Allowlist gates DTPM actuation |
| **WP-N5/N6/N7** | Each domain's states inform budget allocation |
| **WP-N1** | Workload-class priorities drive shedding order |
| **WP-N9** | Thermal/fan budget coupling — adjacent arbitrator |
| **ADR-0013** | Deterministic heuristic, no learned control |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Draft outer-loop algorithm pseudocode in Rust (`crates/optid/src/outer_loop.rs` skeleton)
- [ ] Confirm RAPL + dGPU power observation paths
- [ ] Confirm thermal zone policy writing interface
- [ ] Draft `optctl budget status` subcommand

#### Short-term (needs hardware)
- [ ] Run §4.1 convergence time on Legion 5 (has dGPU for richer test)
- [ ] Run §4.2 priority correctness
- [ ] Run §4.3 cgroup freeze on T14 Gen 4

#### Medium-term
- [ ] Land `--outer-loop=enabled` flag (default `disabled` in v0.x)
- [ ] Promote research from WIP to Validated
- [ ] Update SPEC §4.4 DTPM row to `A`
- [ ] v0.x+1: implement cgroup freezing for floor preservation

### Suggested Reading

#### Kernel source
- `drivers/powercap/intel_rapl.c`
- `drivers/thermal/power_allocator.c`
- `drivers/thermal/dtpm.c`
- `drivers/platform/x86/intel/hfi/` — HFI

#### Documentation
- `Documentation/power/powercap/powercap.rst`
- `Documentation/driver-api/thermal/sysfs-api.rst`

#### Prior art
- Intel `thermald` — `https://github.com/intel/thermal_daemon`
- `power-profiles-daemon` (no outer loop — anti-prior-art)

#### Project-internal
- SPEC §0 (objective), §4.4, §6 WP-N8
- Research 0006, 0008, 0009, 0011 (hard deps)
- Research 0002, 0003

---

