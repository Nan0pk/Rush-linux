# OPTID-COMPLETION-PLAN.md

Full Build-Out of the Missing 70%

**Date:** 2026-07-22  
**Author:** Agent (Qwen3.8)  
**Status:** Proposed

---

## Section 1: Where We Are

### 1.1 What works (the 30%)

- CPU EPP writes (actuator.rs Action::CpuEpp)
- platform_profile writes (actuator.rs Action::PlatformProfile)
- PM QoS /dev/cpu_dma_latency (actuator.rs:52)
- PM QoS per-device resume (actuator.rs:74)
- 5-class workload classifier + vm.guest (workload.rs)
- Policy engine mode resolution (policy.rs Policy::decide())
- 2-second poll loop (main.rs)
- PPD D-Bus shim full (shim/ppd.rs 42025 chars)
- GameMode D-Bus shim full (shim/gamemode.rs 22251 chars)
- Conflict detection (shim/conflict.rs)
- Hardware allowlist WP-N4 (allowlist.rs 23778 chars)
- Backlight selection + floor (actuators/display.rs)
- Runtime PM predicates (actuators/runtime_pm.rs)
- Storage predicates CNVi/ALPM (actuators/storage.rs)
- Contracts table per-class floors (contracts.rs)
- Foreground stub never yields (foreground/mod.rs)
- optctl full CLI (optctl/src/main.rs)
- rush_telemetry standalone NOT integrated
- Revert journal (io_util.rs)
- Capability manifest (capability.rs 33540 chars)

### 1.2 Stub or dead code

- foreground::subscribe() - sleeps forever
- Contracts::fits_contract() - zero call sites
- Snapshot.runtime_pm_device_paths - never actuated
- Snapshot.pcie_aspm_device_paths - never actuated
- Snapshot.sata_alpm_host_paths - never actuated
- Policy.foreground config - parsed but dead_code
- rush_telemetry - zero optid integration

### 1.3 Designed but not implemented (the 70%)

- Foreground real (0005,0018): compositor, login1, app-map
- Event-driven PSI (0004): inotify replaces 2s poll
- PI/PID control (0001,0003): PSI->EPP, thermal->powercap
- Per-cgroup contracts (0003): per-cgroup PSI/budgets
- sched_ext selection (0014): switch per class
- Display PSR/VRR/DPMS (0007): bridge, DRM, hints
- GPU upscaling/ALS (0019): resolution, brightness
- dGPU PM + MUX (0011): suspend, MUX switch
- Storage actuation (0008): APST, ASPM, ALPM writes
- Runtime PM actuation (0009): control=auto writes
- Thermal/fan (0013): zones, watt budget
- Powercap (0012): power_limit_uw
- Memory (0015): zram, MGLRU, swappiness
- eBPF telemetry (0004,0018): wire into optid
- Wakeup/C-state (0018): sources, residency
- policy.toml full-domain (0003+): all domains

### 1.4 Completion: CPU 80%, Workload 60%, Shims 100%, Allowlist 90%, Foreground 10%, Display 20%, RuntimePM 20%, Storage 20%, dGPU/Thermal/Powercap/Memory/sched_ext/GPU/eBPF/PI/cgroup 0%

---

## Section 2: Where We Want To Be

1. Idle: deep C-states, dGPU suspended, 48Hz VRR PSR2, NVMe PS4, 2-4W
2. Interactive: EPP balance_perf, VRR 60-120Hz, NVMe PS0, 5-15W
3. Latency-critical: dGPU wakes, EPP perf, PM QoS 1ms, scx_bpfland, 45-125W
4. Throughput: all cores, scx_rusty, fan max, 65-125W
5. Battery: EPP power, backlight 40%, dGPU suspends, power -30%
6. Thermal: powercap reduces PL1, no oscillation

100% means: every SPEC 3.2 domain has actuator, gated by --apply+allowlist+fits_contract+journal. Event-driven PSI. PI controller inner loop. 1Hz outer loop. Foreground on GNOME/KDE/wlroots. optctl explain shows all. All behind feature flags.

---

## Section 3: Why This Order

- A (fits_contract gate) MUST come first - all depth-enablers need it
- B,C,D independent after A (runtime PM, storage, display)
- E (dGPU) depends on B (dGPU suspend IS runtime PM)
- F (foreground) fully independent - reads D-Bus not sysfs
- G->H sequential (thermal budget feeds powercap)
- I (event-driven PSI + PI) independent of B-H
- J (memory) and K (sched_ext) independent of everything
- L (eBPF) depends on I (telemetry feeds PI error signal)
- M (per-cgroup + full policy) depends on ALL (capstone)

Parallel after A: B,C,D,F,G,I,J,K all independent.
Critical path: A->B->E->(wait I)->L->M
Fully independent: F, J, K

---

## Section 4: The Steps

### Step A: Wire fits_contract() Gate

Starting: fits_contract() in contracts.rs has zero call sites.
Do: Call it in actuator.rs at each depth-enabler match arm. If false, emit Skipped with reason. Read exit latency from pm_qos_resume_latency_us. Test: tight floor + slow device = skip.
Flag: [contracts] enforce=true (default). CLI: --no-contract-gate.
Depends: nothing. Depended: B,C,D,E. Scope: Small <200 LOC.

### Step B: Runtime PM Autosuspend Actuation

Starting: runtime_pm.rs has predicates. Action::RuntimePm exists. Write logic is TODO stub. Paper 0009.
Do: Implement write power/control=auto + autosuspend_delay_ms. Journal. has_active_link skip. Allowlist gate. Contract gate. Emit in policy.rs for battery+idle/light. Per-class delays in config.
Flag: [depth.runtime_pm] enabled=true. CLI: --no-runtime-pm.
Depends: A. Depended: E. Scope: Medium 200-800 LOC.

### Step C: Storage ASPM + SATA ALPM Actuation

Starting: storage.rs has is_cnvi(), DEFAULT_ALPM_POLICY. Action::PcieAspm/SataAlpm exist. Paper 0008.
Do: Write link/l1_aspm 1/0. Skip CNVi. Write ALPM policy. Allowlist+contract gates. Emit for idle/light on battery.
Flag: [depth.storage] aspm/alpm=true. CLI: --no-storage-pm.
Depends: A. Independent of B,D,E,F,G,H,I,J,K. Scope: Small.

### Step D: Display Backlight Actuation

Starting: display.rs has select_backlight(), MIN_FLOOR_PCT=10. Snapshot.selected_backlight populated. Paper 0007.
Do: Add Action::BacklightBrightness. Write brightness file. Clamp to 10% floor. Emit target=40% battery+idle. Restore on interactive.
Flag: [depth.display] backlight_control=true. CLI: --no-backlight.
Depends: A. Independent of B,C,E,F,G,H,I,J,K. Scope: Small.

### Step E: dGPU Runtime PM and MUX Control

Starting: No dgpu.rs exists. Paper 0011. Step B must exist.
Do: Create sensors/dgpu.rs (discover via PCI class 0x0300). Create actuators/dgpu.rs (suspend/wake via runtime PM). MUX via vendor sysfs (trait-based). Emit DgpuSuspend on idle. DgpuWake on latency-critical/GameMode.
Flag: [depth.dgpu] enabled=true. CLI: --no-dgpu-pm.
Depends: B. Independent of C,D,F,G,H,I,J,K. Scope: Medium.
Spec gap: MUX paths vendor-specific. Lenovo + generic fallback.

### Step F: Foreground Detection (Real)

Starting: foreground/mod.rs stub. Paper 0005.
Do: Connect login1 D-Bus. Subscribe SessionNew. Detect compositor. Subscribe focus signal (Mutter/KWin/wlr). Read app_id. Match [foreground.app_map]. Write pin file. Handle no-compositor.
Flag: --foreground=auto|off (default off). [foreground] enabled=false.
Depends: nothing. Fully independent. Scope: Large 800+ LOC.
Spec gap: wlroots protocol varies. Fallback to /proc/pid/cgroup.

### Step G: Thermal and Fan Budget Coupling

Starting: Snapshot.max_temp_millic exists. No per-zone/fan/budget. Paper 0013.
Do: Create sensors/thermal.rs. Read per-zone temps. Read fan speed. Compute thermal_budget_watts (linear derate above threshold). Add to Decision for Step H consumption.
Flag: [thermal] enabled=true, throttle_threshold_c=80. CLI: --no-thermal-budget.
Depends: nothing. Depended: H. Scope: Medium.

### Step H: DTPM/Powercap Outer Loop

Starting: No powercap.rs exists. Paper 0012. Needs Step G budget.
Do: Create sensors/powercap.rs (enumerate intel-rapl domains). Create actuators/powercap.rs (set_power_limit with min/max guards). Emit PowercapLimit when budget < PL1. Restore on recovery. 1Hz loop.
Flag: [depth.powercap] enabled=false (opt-in). CLI: --powercap.
Depends: G. Independent of B,C,D,E,F,I,J,K. Scope: Medium.
Spec gap: AMD HSMP different. Intel RAPL only for v1.

### Step I: Event-Driven PSI + PI Controller

Starting: main.rs thread::sleep(2s). Threshold classifier. Papers 0001,0004.
Do: Replace sleep with inotify on /proc/pressure (500ms min, 5s max). Replace thresholds with PI controller (error=target_psi-current, output=EPP, Kp/Ki configurable, anti-windup). Keep 5-class as outer selector. Read PSI via total= counter (lockless).
Flag: [control] mode=pi|threshold (default threshold). CLI: --pi-control.
Depends: nothing. Depended: L, M. Scope: Large 800+ LOC.
Spec gap: Gains unspecified. Use Kp=0.3 Ki=0.05. Kernel>=5.13 for inotify.

### Step J: Memory Domain (zram + MGLRU)

Starting: policy.rs has MemoryConfig. Snapshot.zram_swap_active. Paper 0015.
Do: Create actuators/memory.rs. configure_zram(ram_gb) sizes by tier. set_mglru(enabled). set_swappiness(value). Emit per class/mode.
Flag: [depth.memory] zram/mglru=false (opt-in). CLI: --memory-tune.
Depends: nothing. Fully independent. Scope: Medium.
Spec gap: zram resize boot-time only. MGLRU kernel>=6.1.

### Step K: sched_ext Scheduler Selection

Starting: No sched.rs. Paper 0014. ADR-0015. scx_loader tool.
Do: Create actuators/sched.rs. current_scheduler(), switch via scx_loader, fallback_to_eevdf(). Emit SchedSwitch per class. Cooldown 30s. Eviction detection -> EEVDF fallback + 10min cooldown.
Flag: [depth.sched_ext] enabled=false. CLI: --sched-ext. Kernel>=6.12.
Depends: nothing. Fully independent. Scope: Medium.
Spec gap: scx_loader CLI may change. Pin version.

### Step L: eBPF Telemetry Integration

Starting: rush_telemetry standalone. Not in optid Cargo.toml. Papers 0004,0018.
Do: Add rush_telemetry as optional dep. Create telemetry.rs. Replace Pressure::read with PsiReader. Add RAPL energy to Snapshot. Add HFI topology. Implement wakeup/C-state/PM-QoS reads. Feed PI controller. Expose via D-Bus Telemetry() method.
Flag: [telemetry] ebpf/rapl=false. CLI: --ebpf-telemetry. Compile: off.
Depends: I. Depended: M. Scope: Large 800+ LOC.
Spec gap: eBPF needs clang+libbpf. Provide non-eBPF fallback.

### Step M: Per-Cgroup Contracts + Full policy.toml

Starting: Contracts global only. policy.toml missing new-domain sections. Paper 0003.
Do: Extend policy.toml with all domain sections. Implement per-cgroup PSI reading from /sys/fs/cgroup/<slice>/cpu.pressure. Extend Contracts for per-cgroup overrides. Wire all actions into decide(). Update optctl explain. Update capability.rs.
Flag: Each domain has own enabled flag. Global --dry-run. Per-domain CLI flags.
Depends: ALL (A-L). Capstone integration. Scope: Large 800+ LOC.
Spec gap: Per-cgroup PSI needs cgroup v2 delegation. Ship systemd drop-in.

---

## Section 5: Dependency Graph

Critical path: A -> B -> E -> (wait I) -> L -> M
Parallel after A: B, C, D, F, G, I, J, K
Fully independent: F, J, K
Sequential: G->H, I->L->M

---

## Section 6: Integration and Testing

Each step = separate branch. Must: build clean, clippy clean, tests pass, flag defaults OFF (except A). Merge: A first, B-K any order, L after I, M last.
No-hardware testing: RUSH_SYSFS_ROOT mock, zbus test transport, EnergyPreference::Disabled, testos VM, --dry-run.

---

## Section 7: Out of Scope

Distro packaging, user docs, marketing, kernel configs, UKI signing, mkosi pinning, benchmark validation runs, GPU upscaling (v2), display-bridge user service (v2).

---

## Section 8: Fit with Existing Plans

corrected-path-forward gates on hardware. This builds code Phase D validates. v0.6 proposal covers Phases A-E only. This covers the 70% after v0.6. Satisfies prerequisites for v0.7/v0.8/v0.9. New milestone needed: 0.8.0-beta.1 Full-Domain optid. Hardware gate blocks STATUS change not merge.

---

## Section 9: Blocking Specs

None are hard blockers. All have safe defaults.
1. PI gains: use Kp=0.3 Ki=0.05
2. MUX paths: Lenovo + generic fallback
3. AMD HSMP: Intel RAPL only v1
4. Fan paths: probe by hwmon name
5. wlroots: fallback to cgroup
6. eBPF build: non-eBPF fallback
7. PWM floor: universal 10%
8. zram resize: boot-time only
9. scx_loader: pin version
10. cgroup delegation: ship drop-in

---

## Agent Execution Notes

Single agent: Start A, then any of B-K. Each step = one PR.
Swarm: Track1(A->B->E), Track4(F), Track5(G->H), Track6(I), Track7+8(J,K). Merge all. Then L->M senior agent.
Less-capable agents: Follow numbered checklists literally. Do not improvise. Implement minimum passing tests. No refactoring.
Verification: cargo build, clippy, test. Flag OFF. No regressions. Commit: feat(optid): <step> - <desc>

---

*End of plan.*
