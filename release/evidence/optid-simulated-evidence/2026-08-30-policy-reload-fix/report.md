# optid simulated evidence — fully enabled versus off

> **Everything in this report is simulated and modelled.** No physical power, battery life, temperature, hardware compatibility or real-world performance was measured, and none is claimed. Every latency, throughput, energy and temperature figure is computed by a documented model from the state of a simulated machine.

**Question.** When optid is fully enabled, does it theoretically improve the modelled system compared with optid off, while remaining safe under faults and recovery?

**Answer.** theoretically beneficial with named regressions

- 10 improved, 47 neutral, 1 worse, 41 uncertain measurements for the fully enabled arm.
- 9 of 10 optid domains produced an action that actually became active on the simulated machine; the rest are reported unsupported.

## Evidence class

`docs/research/0024-non-bare-metal-optid-validation-method.md` requires every result to be named by its evidence class before it is used. This run produces exactly two:

- **Deterministic software proof** — the policy decisions, per-domain gating, allowlist and contract gates, write intent, read-back, restoration, crash handling, recovery and failure behaviour. These are facts about optid's code.
- **Model-conditional estimate** — every latency, throughput, completed-work, pressure, energy and temperature number, and therefore every improvement or regression in this report. These hold *inside the declared model and its parameter range*, and nowhere else.

It produces **no measured guest outcome** and **no physical measurement**. Nothing here supports a claim about laptop watts, battery life, fan behaviour, suspend and resume, firmware compatibility, or support for any named machine. Those claims remain blocked until matching physical evidence exists.


## How this was produced

The unmodified production control loop (`crate::run`) was driven against a simulated machine. Real optid code performed the sensing, workload classification, mode selection, per-domain gating, hardware-allowlist checks, contract checks, capability checks, transactional actuation, journalling, circuit-breaker accounting, shutdown restoration and startup recovery. Only the machine underneath is modelled.

- Simulation root: `<simulation root>`
- Simulated machine: rush-sim-laptop-a (4 CPUs, 6 devices, backlight intel_backlight max=960)
- Arms: 15
- Scenarios: 15
- Repeats per arm/scenario: 3
- Write attempts that left the simulation root: **0**

Containment guards in force:

- PATH restricted to an empty directory inside the simulation root (<simulation root>/guard/empty-bin), so no `systemctl` or any other host binary can be executed
- DBUS_SYSTEM_BUS_ADDRESS and DBUS_SESSION_BUS_ADDRESS point at a non-existent socket inside the simulation root (<simulation root>/guard/no-system-bus), so no system service can be reached or claimed
- a process-wide SIGTERM flag handler is installed before any run, so the harness's own clean-shutdown signal can never terminate the process
- the competing-daemon detector is pinned to a deterministic "no conflict" answer, so it never spawns a process
- the only process the harness starts is the sibling `optid-recover` executable, by absolute path, with `--machine-root` pointing inside the simulation root

## 1. Where fully enabled optid improved the modelled result

**idle_battery**

- `energy_j` +39.6% (baseline 174.349, enabled 105.346; assumption range +33.7%..+46.9%)
- `mean_power_w` +39.6% (baseline 10.897, enabled 6.584; assumption range +33.7%..+46.9%)

**memory_pressure_ac**

- `io_stall_pct` +58.2% (baseline 15.112, enabled 6.315; assumption range +46.8%..+58.2%)

**mixed_foreground_background_battery**

- `energy_j` +12.8% (baseline 508.159, enabled 443.032; assumption range +4.2%..+18.9%)
- `mean_power_w` +12.8% (baseline 28.231, enabled 24.613; assumption range +4.2%..+18.9%)

**storage_pressure_ac**

- `foreground_p99_latency_us` +27.4% (baseline 4366.895, enabled 3169.895; assumption range +5.7%..+34.0%)
- `foreground_mean_latency_us` +23.8% (baseline 2515.819, enabled 1917.319; assumption range +4.7%..+28.3%)
- `io_stall_pct` +46.3% (baseline 59.945, enabled 32.211; assumption range +42.7%..+46.3%)

**thermal_rise_and_recovery_ac**

- `throughput_ops_per_s` +5.9% (baseline 1280.064, enabled 1355.004; assumption range +2.0%..+12.6%)

**throughput_ac**

- `throughput_ops_per_s` +4.6% (baseline 1051.212, enabled 1099.423; assumption range +2.1%..+9.0%)


## 2. Where it made no meaningful difference

- **ac_to_battery_and_back**: cpu_stall_pct, memory_stall_pct, io_stall_pct, peak_die_temp_c, completed_work_units, mean_die_temp_c
- **idle_battery**: foreground_p99_latency_us, foreground_mean_latency_us, cpu_stall_pct, memory_stall_pct, io_stall_pct, peak_die_temp_c, throughput_ops_per_s, completed_work_units, mean_die_temp_c
- **interactive_ac**: cpu_stall_pct, memory_stall_pct, io_stall_pct, peak_die_temp_c, throughput_ops_per_s, completed_work_units, mean_die_temp_c
- **latency_critical_ac**: cpu_stall_pct, memory_stall_pct, peak_die_temp_c, throughput_ops_per_s, completed_work_units, mean_die_temp_c
- **memory_pressure_ac**: cpu_stall_pct, energy_j, mean_power_w, peak_die_temp_c, mean_die_temp_c
- **mixed_foreground_background_battery**: memory_stall_pct, peak_die_temp_c, mean_die_temp_c
- **storage_pressure_ac**: cpu_stall_pct, memory_stall_pct, peak_die_temp_c, completed_work_units, mean_die_temp_c
- **thermal_rise_and_recovery_ac**: memory_stall_pct, peak_die_temp_c, mean_die_temp_c
- **throughput_ac**: memory_stall_pct, peak_die_temp_c, mean_die_temp_c

A change smaller than 2% of the baseline is reported as no meaningful difference.

## 3. Where it made the modelled result worse

**mixed_foreground_background_battery**

- `throughput_ops_per_s` -11.5% (baseline 1239.643, enabled 1096.924; assumption range -21.3%..-2.5%)


## 4. Which optid actions caused each change

**ac_to_battery_and_back**

- `vm_sysctl` → foreground_p99_latency_us +2.2% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**idle_battery**

- `backlight` → energy_j +31.1%; mean_power_w +31.1%
  - backlight:intel_backlight: 960 -> 384 (read back 384, restored 960)
- `pci_aspm` → energy_j +3.2% (assumption-sensitive); mean_power_w +3.2% (assumption-sensitive)
  - pci_aspm:0000:01:00.0: 0 -> 1 (read back 1, restored 0)
  - pci_aspm:0000:02:00.0: 0 -> 1 (read back 0, restored 0)
- `sata_alpm` → energy_j +3.9% (assumption-sensitive); mean_power_w +3.9% (assumption-sensitive)
  - sata_alpm:host0: max_performance -> med_power_with_dipm (read back med_power_with_dipm, restored max_performance)

**interactive_ac**

- `vm_sysctl` → foreground_p99_latency_us +2.7% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**latency_critical_ac**

- `vm_sysctl` → foreground_p99_latency_us +4.3% (assumption-sensitive); foreground_mean_latency_us +4.0% (assumption-sensitive); io_stall_pct +66.2% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**memory_pressure_ac**

- `vm_sysctl` → foreground_p99_latency_us +8.6% (assumption-sensitive); foreground_mean_latency_us +7.7% (assumption-sensitive); memory_stall_pct +9.1% (assumption-sensitive); io_stall_pct +58.2%; throughput_ops_per_s +3.0% (assumption-sensitive); completed_work_units +3.7% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**mixed_foreground_background_battery**

- `backlight` → energy_j +2.7%; mean_power_w +2.7%
  - backlight:intel_backlight: 960 -> 384 (read back 384, restored 960)
- `cpu_epp` → cpu_stall_pct -612.3% (assumption-sensitive); energy_j +6.5% (assumption-sensitive); mean_power_w +6.5% (assumption-sensitive); throughput_ops_per_s -6.9% (assumption-sensitive); completed_work_units -6.1% (assumption-sensitive)
  - cpu_epp:cpu0: balance_performance -> power (read back power, restored balance_performance)
  - cpu_epp:cpu1: balance_performance -> power (read back power, restored balance_performance)
  - cpu_epp:cpu2: balance_performance -> power (read back power, restored balance_performance)
  - cpu_epp:cpu3: balance_performance -> power (read back power, restored balance_performance)
- `platform_profile` → foreground_p99_latency_us -2.9% (assumption-sensitive); cpu_stall_pct -343.7% (assumption-sensitive); energy_j +3.4% (assumption-sensitive); mean_power_w +3.4% (assumption-sensitive); throughput_ops_per_s -4.8% (assumption-sensitive); completed_work_units -3.4% (assumption-sensitive)
  - platform_profile:acpi: balanced -> low-power (read back low-power, restored balanced)
- `sata_alpm` → throughput_ops_per_s -2.3% (assumption-sensitive)
  - sata_alpm:host0: max_performance -> med_power_with_dipm (read back med_power_with_dipm, restored max_performance)
- `vm_sysctl` → foreground_p99_latency_us +4.1% (assumption-sensitive); foreground_mean_latency_us +3.8% (assumption-sensitive); io_stall_pct +63.2% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**storage_pressure_ac**

- `vm_sysctl` → foreground_p99_latency_us +27.4%; foreground_mean_latency_us +23.8%; io_stall_pct +46.3%; throughput_ops_per_s +7.0% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**thermal_rise_and_recovery_ac**

- `cpu_epp` → cpu_stall_pct +7.1% (assumption-sensitive); energy_j -2.7% (assumption-sensitive); mean_power_w -2.7% (assumption-sensitive); throughput_ops_per_s +2.2% (assumption-sensitive); completed_work_units +2.3% (assumption-sensitive)
  - cpu_epp:cpu0: balance_performance -> performance (read back performance, restored balance_performance)
  - cpu_epp:cpu1: balance_performance -> performance (read back performance, restored balance_performance)
  - cpu_epp:cpu2: balance_performance -> performance (read back performance, restored balance_performance)
  - cpu_epp:cpu3: balance_performance -> performance (read back performance, restored balance_performance)
- `platform_profile` → cpu_stall_pct +7.1% (assumption-sensitive); energy_j -2.3% (assumption-sensitive); mean_power_w -2.3% (assumption-sensitive); throughput_ops_per_s +2.2% (assumption-sensitive); completed_work_units +2.3% (assumption-sensitive)
  - platform_profile:acpi: balanced -> performance (read back performance, restored balanced)
- `vm_sysctl` → foreground_p99_latency_us +3.1% (assumption-sensitive); foreground_mean_latency_us +2.8% (assumption-sensitive); io_stall_pct +64.8% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)

**throughput_ac**

- `cpu_epp` → energy_j -2.8% (assumption-sensitive); mean_power_w -2.8% (assumption-sensitive)
  - cpu_epp:cpu0: balance_performance -> performance (read back performance, restored balance_performance)
  - cpu_epp:cpu1: balance_performance -> performance (read back performance, restored balance_performance)
  - cpu_epp:cpu2: balance_performance -> performance (read back performance, restored balance_performance)
  - cpu_epp:cpu3: balance_performance -> performance (read back performance, restored balance_performance)
- `platform_profile` → energy_j -2.4% (assumption-sensitive); mean_power_w -2.4% (assumption-sensitive)
  - platform_profile:acpi: balanced -> performance (read back performance, restored balanced)
- `vm_sysctl` → foreground_p99_latency_us +4.1% (assumption-sensitive); foreground_mean_latency_us +3.6% (assumption-sensitive); io_stall_pct +62.4% (assumption-sensitive)
  - vm_sysctl:dirty_background_bytes: 0 -> 67108864 (read back 67108864, restored 0)
  - vm_sysctl:dirty_bytes: 0 -> 134217728 (read back 134217728, restored 0)
  - vm_sysctl:swappiness: 60 -> 100 (read back 100, restored 60)


## 5. Did a combined configuration hide a harmful action?

Yes. The following individually harmful actions do not show as harmful once every domain runs together:

- platform_profile alone makes foreground_p99_latency_us worse by 2.9%, but the fully enabled configuration reports NoMeaningfulDifference (+1.0%). The combined result hides this action.

## 6. Did every successful action restore correctly?

- 783 receipts recorded; 747 actions became active on the simulated machine; 742 of those restored to their previous value.
- **In every scenario with no injected fault, every action that became active restored to its previous value.**
- Actions left applied by an injected fault (the fault is the cause, and each one is the behaviour that fault is meant to produce):

  - full_enabled / failed_restoration: platform_profile:acpi ended at performance instead of balanced
  - full_stock_allowlist / failed_restoration: platform_profile:acpi ended at performance instead of balanced
  - harmful_control / write_failures_and_circuit: cpu_epp:cpu0 ended at balance_p instead of balance_performance
  - harmful_control / failed_restoration: platform_profile:acpi ended at low-power instead of balanced
  - only_platform_profile / failed_restoration: platform_profile:acpi ended at performance instead of balanced
- Crash and reboot recovery exercised: true. Everything restored after recovery: true.
- Injected failed-restoration scenario detected as a restoration failure: true.
- Crash recovery was exercised through the production daemon restart path (reconciler hydrate, journal replay and handback), after clearing the tmpfs state directory to simulate a reboot.
- The standalone S3D `optid-recover` executable was run as a real subprocess before every supervised restart, matching `optid-apply.service`'s `Requires=optid-recover.service`. Its `--machine-root` flag, which exists only in a `test-simulation` build, rebases every recorded target path into the simulated machine, so no recovery write can reach a host path.
- The `cgroup_reweight` domain is held off in every arm: actuating it means executing `systemctl` against real system services, which this harness must never do. It is reported as unsupported in simulation, not as a passing test.

## 7. What is unsupported or too assumption-sensitive to judge

**Domains that actually actuated, per configuration.** A domain that never produced an action which read back as requested is unsupported here — it is not a passing test.

- `full_enabled`: backlight, cpu_dma_latency, cpu_epp, device_resume_latency, pci_aspm, platform_profile, runtime_pm, sata_alpm, vm_sysctl
- `full_stock_allowlist`: cpu_dma_latency, cpu_epp, platform_profile, vm_sysctl
- `harmful_control`: backlight, cpu_dma_latency, cpu_epp, device_resume_latency, pci_aspm, platform_profile, runtime_pm, sata_alpm, vm_sysctl
- `only_backlight`: backlight
- `only_cpu_dma_latency`: cpu_dma_latency
- `only_cpu_epp`: cpu_epp
- `only_device_resume_latency`: device_resume_latency
- `only_pci_aspm`: pci_aspm
- `only_platform_profile`: platform_profile
- `only_runtime_pm`: runtime_pm
- `only_sata_alpm`: sata_alpm
- `only_vm_sysctl`: vm_sysctl

**Domains with no action that became active anywhere:**

- `cgroup_reweight` — held off in every arm: actuating it would execute `systemctl` against real system services

**Inert controls** (the write was accepted and the machine ignored it; treated as unsupported, never as a passing test): pci_aspm:0000:02:00.0

**Controls that refused every write:** runtime_pm_control:0000:00:1c.0

**Rejected results:**

- full_enabled / failed_restoration: platform_profile:acpi: became active and did not restore (previous=balanced, ended at performance)
- full_stock_allowlist / failed_restoration: platform_profile:acpi: became active and did not restore (previous=balanced, ended at performance)
- harmful_control / failed_restoration: platform_profile:acpi: became active and did not restore (previous=balanced, ended at low-power)
- harmful_control / write_failures_and_circuit: cpu_epp:cpu0: became active and did not restore (previous=balance_performance, ended at balance_p)
- only_platform_profile / failed_restoration: platform_profile:acpi: became active and did not restore (previous=balanced, ended at performance)

**Assumption-sensitive measurements:**

- interactive_ac / foreground_p99_latency_us (+2.7%, range +0.0%..+21.7%) — assumption-sensitive
- interactive_ac / foreground_mean_latency_us (+1.9%, range +0.0%..+12.4%) — assumption-sensitive
- interactive_ac / energy_j (+0.0%, range -8.6%..+0.0%) — assumption-sensitive
- interactive_ac / mean_power_w (+0.0%, range -8.6%..+0.0%) — assumption-sensitive
- latency_critical_ac / foreground_p99_latency_us (+4.3%, range +0.6%..+26.4%) — assumption-sensitive
- latency_critical_ac / foreground_mean_latency_us (+4.0%, range +0.0%..+20.4%) — assumption-sensitive
- latency_critical_ac / io_stall_pct (+66.2%, range +0.0%..+66.2%) — assumption-sensitive
- latency_critical_ac / energy_j (+0.0%, range -3.4%..+0.0%) — assumption-sensitive
- latency_critical_ac / mean_power_w (+0.0%, range -3.4%..+0.0%) — assumption-sensitive
- throughput_ac / foreground_p99_latency_us (+4.1%, range +0.6%..+4.3%) — assumption-sensitive
- throughput_ac / foreground_mean_latency_us (+3.6%, range +0.5%..+3.7%) — assumption-sensitive
- throughput_ac / cpu_stall_pct (+5.3%, range +0.0%..+10.6%) — assumption-sensitive
- throughput_ac / io_stall_pct (+62.4%, range +0.0%..+62.4%) — assumption-sensitive
- throughput_ac / energy_j (-5.5%, range -11.7%..-1.0%) — assumption-sensitive
- throughput_ac / mean_power_w (-5.5%, range -11.7%..-1.0%) — assumption-sensitive
- throughput_ac / completed_work_units (+3.1%, range +0.5%..+7.8%) — assumption-sensitive
- memory_pressure_ac / foreground_p99_latency_us (+8.6%, range +1.3%..+19.3%) — assumption-sensitive
- memory_pressure_ac / foreground_mean_latency_us (+7.7%, range +1.2%..+15.0%) — assumption-sensitive
- memory_pressure_ac / memory_stall_pct (+9.1%, range +0.0%..+9.1%) — assumption-sensitive
- memory_pressure_ac / throughput_ops_per_s (+3.0%, range +0.4%..+3.1%) — assumption-sensitive
- memory_pressure_ac / completed_work_units (+3.7%, range +0.6%..+3.7%) — assumption-sensitive
- storage_pressure_ac / energy_j (+0.0%, range -4.8%..+0.0%) — assumption-sensitive
- storage_pressure_ac / mean_power_w (+0.0%, range -4.8%..+0.0%) — assumption-sensitive
- storage_pressure_ac / throughput_ops_per_s (+7.0%, range +1.0%..+7.1%) — assumption-sensitive
- mixed_foreground_background_battery / foreground_p99_latency_us (+1.0%, range -3.2%..+7.2%) — assumption-sensitive
- mixed_foreground_background_battery / foreground_mean_latency_us (+2.0%, range -1.6%..+6.4%) — assumption-sensitive
- mixed_foreground_background_battery / cpu_stall_pct (+9.679 in metric units; the baseline 0.000 is below the noise floor, so a percentage would be misleading) — assumption-sensitive
- mixed_foreground_background_battery / io_stall_pct (+56.9%, range +0.0%..+62.0%) — assumption-sensitive
- mixed_foreground_background_battery / completed_work_units (-9.7%, range -20.5%..+0.0%) — assumption-sensitive
- thermal_rise_and_recovery_ac / foreground_p99_latency_us (+3.1%, range +0.5%..+3.3%) — assumption-sensitive
- thermal_rise_and_recovery_ac / foreground_mean_latency_us (+2.8%, range +0.4%..+3.0%) — assumption-sensitive
- thermal_rise_and_recovery_ac / cpu_stall_pct (+14.3%, range +0.0%..+26.3%) — assumption-sensitive
- thermal_rise_and_recovery_ac / io_stall_pct (+64.8%, range +0.0%..+64.8%) — assumption-sensitive
- thermal_rise_and_recovery_ac / energy_j (-5.2%, range -10.6%..-1.0%) — assumption-sensitive
- thermal_rise_and_recovery_ac / mean_power_w (-5.2%, range -10.6%..-1.0%) — assumption-sensitive
- thermal_rise_and_recovery_ac / completed_work_units (+4.7%, range +0.8%..+11.7%) — assumption-sensitive
- ac_to_battery_and_back / foreground_p99_latency_us (+0.0%, range -4.6%..+21.4%) — assumption-sensitive
- ac_to_battery_and_back / foreground_mean_latency_us (+0.7%, range -1.2%..+11.7%) — assumption-sensitive
- ac_to_battery_and_back / energy_j (+1.4%, range -9.8%..+1.8%) — assumption-sensitive
- ac_to_battery_and_back / mean_power_w (+1.4%, range -9.8%..+1.8%) — assumption-sensitive

## Findings the comparison table does not show

### `owned_target_hot_removal_aborts_the_control_loop` — medium

Removing a device optid owns makes the reconciler's transaction target canonicalisation fail, and the error propagates out of the control loop. The loop exits before its shutdown handback, so every owned target stays applied. The first supervised restart then refuses to start at all (StaleGeneration on the vanished target's record); only the S3D `optid-recover` pass that runs before the next restart clears it. Two restarts and one recovery pass are needed to hand the machine back, and `optid-apply.service` allows three starts per minute.

- full_enabled / hotplug_device_and_cpu: error: JournalIo: canonicalize transaction target: NotFound: No such file or directory (os error 2) (Other) (recovery: error: StaleGeneration: device-resume:20a110dbe1b89629 belongs to generation 000000000000000018d07fba1c9e2bcf-000007dc, not 000000000000000018d07fba20cc0189-000007dc (Other) -> clean)
- harmful_control / hotplug_device_and_cpu: error: JournalIo: canonicalize transaction target: NotFound: No such file or directory (os error 2) (Other) (recovery: error: StaleGeneration: device-resume:20a110dbe1b89629 belongs to generation 000000000000000018d07fba9b0dfdd9-000007dc, not 000000000000000018d07fba9e7c61d1-000007dc (Other) -> clean)
- only_device_resume_latency / hotplug_device_and_cpu: error: JournalIo: canonicalize transaction target: NotFound: No such file or directory (os error 2) (Other) (recovery: error: StaleGeneration: device-resume:20a110dbe1b89629 belongs to generation 000000000000000018d07fbb2b57cde4-000007dc, not 000000000000000018d07fbb2d50e154-000007dc (Other) -> clean)

### `controls_never_attempted_by_the_fully_enabled_arm` — informational

The simulated machine exposes these controls and the fully enabled configuration never attempted to write any of them in any scenario.

- device_resume_latency:0000:00:14.3
- device_resume_latency:1-4
- pci_aspm:0000:00:14.3
- pci_aspm:0000:00:1f.6
- runtime_pm_control:0000:00:14.3
- runtime_pm_control:0000:00:1f.6
- runtime_pm_delay:0000:00:14.3
- runtime_pm_delay:0000:00:1f.6


## 8. What the complete simulated evidence supports

**theoretically beneficial with named regressions**

No blocking failure was found in the evidence system itself: results were deterministic across 3 repeats, the no-change control did not move the machine, the deliberately harmful control was detected as harmful, and no write escaped the simulation root.

This verdict is about a **modelled** system. It is evidence that the optid design is or is not internally coherent and safe under the modelled assumptions. It is not evidence about any physical machine, and it does not substitute for hardware validation.

## Controls

- **No-change control** (off_all_domains, full_observe): held — no control value moved and no write left the simulated machine.
- **Deliberately harmful control** (harmful_control): detected as harmful.
  - interactive_ac / foreground_p99_latency_us: 5.1% worse
  - interactive_ac / throughput_ops_per_s: 3.0% worse
  - latency_critical_ac / foreground_p99_latency_us: 19.0% worse
  - latency_critical_ac / foreground_mean_latency_us: 8.5% worse
  - latency_critical_ac / throughput_ops_per_s: 12.4% worse
  - throughput_ac / cpu_stall_pct: 42.4% worse
  - throughput_ac / throughput_ops_per_s: 23.1% worse
  - throughput_ac / completed_work_units: 25.2% worse
  - memory_pressure_ac / foreground_p99_latency_us: 4.7% worse
  - memory_pressure_ac / cpu_stall_pct: 1200.0% worse
  - memory_pressure_ac / memory_stall_pct: 13.6% worse
  - memory_pressure_ac / throughput_ops_per_s: 15.1% worse

## Determinism

225 of 225 arm/scenario groups produced byte-identical results across 3 repeats.

## Model assumptions and sensitivity

Every number in this report comes from the model in `crates/optid/src/sim_evidence/model.rs`. The model reads only the simulated machine's control values and the modelled environment. It has no knowledge of which arm is running.

Results are reported under the `nominal` assumption set and re-evaluated under every set below. A result whose direction reverses anywhere in this grid is reported as assumption-sensitive rather than as a finding.

- **nominal** — Mid-range 15 W-class mobile x86 laptop, flash storage, single internal panel. Central estimates.
- **epp_effect_weak** — EPP and platform profile barely move performance or power (firmware ignores most of the hint).
- **epp_effect_strong** — EPP and platform profile move performance and power strongly (aggressive firmware).
- **idle_depth_cheap** — Deep idle is shallow-exit and barely cheaper than a shallow state, so a PM QoS floor costs almost no energy.
- **idle_depth_expensive** — Deep idle saves a lot and exits slowly, so a PM QoS floor is a large energy cost and a large latency win.
- **link_pm_weak** — Link power management saves little and costs little (ASPM/ALPM barely engage).
- **link_pm_strong** — Link power management saves a lot and costs a lot of wake latency.
- **thermal_tight** — Small chassis: high thermal resistance, fast time constant, early throttle.
- **thermal_loose** — Large chassis: low thermal resistance, slow time constant, late throttle.
- **display_dominant** — Bright, power-hungry panel dominates the platform power budget.
- **memory_reclaim_weak** — Swappiness barely changes reclaim behaviour; dirty limits barely change writeback.

**Stated limitation.** The sensitivity grid re-evaluates the recorded machine trajectory. It does not re-run the closed loop, so it bounds model-parameter uncertainty, not uncertainty in optid's own decision trajectory.

**Other stated assumptions.**
- One simulated machine shape is used throughout, so machine-to-machine variation is not covered.
- Workload demand is exogenous: the offered load does not react to how fast the machine serves it.
- Modelled temperature uses a single-node thermal model with one time constant.
- Battery drain is modelled from mean power and a fixed pack capacity; no charge chemistry is modelled.
- The meaningfulness threshold is 2% relative change.

## Scenario catalogue

- **idle_battery** — Screen on, nothing running, on battery. The classic case for depth. (8 cycles of 2s, workload `idle`)
- **interactive_ac** — Desktop use on AC: editing, browsing, moderate wakeups. (8 cycles of 2s, workload `interactive`)
- **latency_critical_ac** — Latency-critical foreground work on AC with a high wakeup rate. (8 cycles of 2s, workload `latency-critical`)
- **throughput_ac** — Sustained compile-style throughput on AC; the machine is CPU bound. (8 cycles of 2s, workload `throughput`)
- **memory_pressure_ac** — Working set larger than resident memory; reclaim is on the critical path. (8 cycles of 2s, workload `memory-pressure`)
- **storage_pressure_ac** — Storage-bound work: high IOPS and heavy writeback. (8 cycles of 2s, workload `storage-pressure`)
- **mixed_foreground_background_battery** — Background work throughout, with a latency-critical foreground application arriving and leaving (the GameMode / foreground pin path). (9 cycles of 2s, workload `mixed-background`)
- **thermal_rise_and_recovery_ac** — Sustained load with an external heat source that drives the die into the throttle band, then is removed so the machine recovers. (12 cycles of 4s, workload `thermal`)
- **ac_to_battery_and_back** — The charger is unplugged mid-run and plugged back in. (12 cycles of 2s, workload `light`)
- **hotplug_device_and_cpu** *(safety)* — Device and CPU hotplug under a running daemon; the capability topology changes twice. (13 cycles of 2s, workload `idle`)
- **config_reload_failure_and_recovery** *(safety)* — policy.toml is replaced with unparseable content under the running daemon, then repaired. (9 cycles of 2s, workload `interactive`)
- **write_failures_and_circuit** *(safety)* — A kernel control starts refusing writes, a second is truncated, and a third drifts under optid. The circuit breaker is expected to open. (10 cycles of 2s, workload `idle`)
  - fault: writes to /sys/firmware/acpi/platform_profile are refused from cycle 2
  - fault: the write to /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference is truncated at cycle 3
  - fault: a third party sets /proc/sys/vm/swappiness to 1 at cycle 4
- **crash_before_restore** *(safety)* — The daemon dies mid-run without restoring. A later start must find the journal and hand the machine back. (8 cycles of 2s, workload `idle`)
  - fault: the daemon dies without restoring after cycle 3
- **failed_restoration** *(safety)* — Restoration itself fails at shutdown: the control refuses the handback write. The workload drives the platform profile away from its power-on value, so there is a real handback to refuse. (8 cycles of 2s, workload `throughput`)
  - fault: restoration of /sys/firmware/acpi/platform_profile is refused at shutdown
- **sensor_loss_and_malformed** *(safety)* — The die sensor disappears and a pressure file goes unparseable while the daemon is running. (8 cycles of 2s, workload `interactive`)
  - fault: /sys/class/hwmon/hwmon0/temp1_input disappears at cycle 3
  - fault: /proc/pressure/cpu becomes unparseable at cycle 5

## Arm catalogue

- **off_absent** — Baseline: optid is not running at all. The machine keeps its power-on control values for the whole trajectory.
- **off_all_domains** — No-change control: the real daemon runs with every domain off. Any control value that moves here is a defect in the off path.
- **full_enabled** — Fully enabled: every supported domain may actuate together, with the simulated hardware verified by an administrator allowlist override.
- **full_stock_allowlist** — Fully enabled with the shipped seeded allowlist and no administrator override — the configuration a real installation starts in.
- **full_observe** — Every domain in observe mode: optid computes the same decisions and suppresses every write.
- **harmful_control** — Deliberately harmful control: a policy whose mode table asks for the worst plausible values for the workload in force.
- **only_cpu_epp** — Isolation arm: only the cpu_epp domain may actuate; every other domain is off.
- **only_platform_profile** — Isolation arm: only the platform_profile domain may actuate; every other domain is off.
- **only_vm_sysctl** — Isolation arm: only the vm_sysctl domain may actuate; every other domain is off.
- **only_cpu_dma_latency** — Isolation arm: only the cpu_dma_latency domain may actuate; every other domain is off.
- **only_device_resume_latency** — Isolation arm: only the device_resume_latency domain may actuate; every other domain is off.
- **only_runtime_pm** — Isolation arm: only the runtime_pm domain may actuate; every other domain is off.
- **only_pci_aspm** — Isolation arm: only the pci_aspm domain may actuate; every other domain is off.
- **only_sata_alpm** — Isolation arm: only the sata_alpm domain may actuate; every other domain is off.
- **only_backlight** — Isolation arm: only the backlight domain may actuate; every other domain is off.

---

Machine-readable evidence: `evidence-bundle.json` (schema 1).
