//! The adaptive policy engine.
//!
//! `Policy` owns the deterministic, explainable decision logic that maps a
//! `Snapshot` to a `Decision`. Per `SPEC-northstar.md` §2, this module is a
//! **CONTRACT-SETTER**: it selects the active workload class, resolves the
//! active mode, and emits the `Action` set that `actuator::Actuator` will
//! apply behind the §3 actuation rule.
//!
//! The decision logic is intentionally a pure function of `(policy, snapshot,
//! override_mode, contracts)` — no I/O, no clocks, no global state. This is
//! what makes `optctl explain` truthful: every reason in the rendered report
//! is reproducible from the inputs.

use std::fs;
use std::path::{Path, PathBuf};

use crate::action::Action;
use crate::contracts::Contracts;
use crate::decision::Decision;
use crate::sensors::Snapshot;
use crate::workload::{Mode, WorkloadClass};

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct MemoryConfig {
    pub(crate) high_swappiness_requires_zram: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Policy {
    pub(crate) thresholds: Thresholds,
    pub(crate) modes: Modes,
    pub(crate) memory: MemoryConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Thresholds {
    pub(crate) cpu_pressure_perf_avg10: f32,
    pub(crate) memory_pressure_protect_avg10: f32,
    pub(crate) io_pressure_throttle_avg10: f32,
    pub(crate) hot_temp_c: f32,
    pub(crate) critical_temp_c: f32,
    pub(crate) low_battery_pct: u8,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Modes {
    pub(crate) battery: ModeConfig,
    pub(crate) balanced: ModeConfig,
    pub(crate) performance: ModeConfig,
    pub(crate) realtime: ModeConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct ModeConfig {
    pub(crate) cpu_epp: String,
    pub(crate) platform_profile: String,
    #[serde(default)]
    pub(crate) background_cpu_weight: Option<u32>,
    #[serde(default)]
    pub(crate) background_io_weight: Option<u32>,
    #[serde(default)]
    pub(crate) user_cpu_weight: Option<u32>,
    #[serde(default)]
    pub(crate) user_io_weight: Option<u32>,
    #[serde(default)]
    pub(crate) requires_controlled_rt_access: Option<bool>,
    #[serde(default)]
    pub(crate) vm_swappiness: Option<u32>,
    #[serde(default)]
    pub(crate) vm_dirty_background_bytes: Option<u64>,
    #[serde(default)]
    pub(crate) vm_dirty_bytes: Option<u64>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            thresholds: Thresholds {
                cpu_pressure_perf_avg10: 12.0,
                memory_pressure_protect_avg10: 5.0,
                io_pressure_throttle_avg10: 8.0,
                hot_temp_c: 82.0,
                critical_temp_c: 92.0,
                low_battery_pct: 20,
            },
            modes: Modes {
                battery: ModeConfig {
                    cpu_epp: "power".to_string(),
                    platform_profile: "low-power".to_string(),
                    background_cpu_weight: Some(25),
                    background_io_weight: Some(25),
                    user_cpu_weight: None,
                    user_io_weight: None,
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(60),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                balanced: ModeConfig {
                    cpu_epp: "balance_performance".to_string(),
                    platform_profile: "balanced".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(150),
                    user_io_weight: Some(150),
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(100),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                performance: ModeConfig {
                    cpu_epp: "performance".to_string(),
                    platform_profile: "performance".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(200),
                    user_io_weight: Some(200),
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(150),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                realtime: ModeConfig {
                    cpu_epp: "performance".to_string(),
                    platform_profile: "performance".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(250),
                    user_io_weight: Some(200),
                    requires_controlled_rt_access: Some(true),
                    vm_swappiness: Some(10),
                    vm_dirty_background_bytes: None,
                    vm_dirty_bytes: None,
                },
            },
            memory: MemoryConfig {
                high_swappiness_requires_zram: true,
            },
        }
    }
}

impl Policy {
    /// Load a `Policy` from a TOML file at `path`. Missing or unparseable
    /// files fall back to `Policy::default()` so a corrupt policy can never
    /// break the daemon — it only loses overrides.
    pub(crate) fn load(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "optid: failed to read policy TOML from {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                return Self::default();
            }
        };

        match toml::from_str(&text) {
            Ok(policy) => policy,
            Err(e) => {
                eprintln!(
                    "optid: failed to parse policy TOML from {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    /// Classify the current snapshot into one of the five SPEC §1 workload
    /// classes. Pure function. Highest precedence: explicit pins (global pin
    /// beats foreground pin beats telemetry).
    pub(crate) fn classify(&self, snapshot: &Snapshot) -> (WorkloadClass, String) {
        if let Some(pinned) = snapshot.global_pinned_class {
            return (pinned, "pinned override (global)".to_string());
        }
        if let Some(pinned) = snapshot.pinned_class {
            return (pinned, "pinned override for foreground app".to_string());
        }

        let load = snapshot.loadavg_1.unwrap_or(0.0);
        let cpu_pressure = snapshot.cpu_pressure.map(|p| p.avg10).unwrap_or(0.0);
        let mem_pressure = snapshot.memory_pressure.map(|p| p.avg10).unwrap_or(0.0);
        let io_pressure = snapshot.io_pressure.map(|p| p.avg10).unwrap_or(0.0);

        if load >= 4.0
            && (cpu_pressure >= self.thresholds.cpu_pressure_perf_avg10
                || io_pressure >= self.thresholds.io_pressure_throttle_avg10)
        {
            return (
                WorkloadClass::Throughput,
                format!(
                    "high load ({:.2}) and high pressure (cpu: {:.2}, io: {:.2})",
                    load, cpu_pressure, io_pressure
                ),
            );
        }

        if (1.5..4.0).contains(&load)
            && cpu_pressure >= self.thresholds.cpu_pressure_perf_avg10
            && snapshot.on_ac == Some(true)
        {
            return (
                WorkloadClass::LatencyCritical,
                format!(
                    "moderate load ({:.2}) with cpu pressure ({:.2}) on AC",
                    load, cpu_pressure
                ),
            );
        }

        if load >= 0.5 || cpu_pressure > 2.0 || mem_pressure > 2.0 {
            return (
                WorkloadClass::Interactive,
                format!(
                    "active usage: load={:.2}, cpu_pressure={:.2}, mem_pressure={:.2}",
                    load, cpu_pressure, mem_pressure
                ),
            );
        }

        if load > 0.05 || cpu_pressure > 0.1 {
            return (
                WorkloadClass::Light,
                format!(
                    "low activity: load={:.2}, cpu_pressure={:.2}",
                    load, cpu_pressure
                ),
            );
        }

        (
            WorkloadClass::Idle,
            format!(
                "system idle: load={:.2}, cpu_pressure={:.2}",
                load, cpu_pressure
            ),
        )
    }

    #[allow(dead_code)]
    pub(crate) fn decide(
        &self,
        snapshot: &Snapshot,
        requested: Mode,
        workload_class: WorkloadClass,
        workload_reason: String,
        contracts: &Contracts,
    ) -> Decision {
        self.decide_resolved(
            snapshot,
            requested,
            workload_class,
            workload_reason,
            contracts,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decide_resolved(
        &self,
        snapshot: &Snapshot,
        requested: Mode,
        workload_class: WorkloadClass,
        workload_reason: String,
        contracts: &Contracts,
        resolved_mode: Option<Mode>,
        mode_hysteresis_reason: Option<String>,
    ) -> Decision {
        let effective_mode = resolved_mode.unwrap_or_else(|| match requested {
            Mode::Auto => self.auto_mode(snapshot),
            explicit => explicit,
        });

        let mut reasons = Vec::new();
        let mut actions = Vec::new();

        if let Some(reason) = mode_hysteresis_reason {
            reasons.push(reason);
        }

        if requested != Mode::Auto {
            reasons.push(format!("manual mode override: {requested}"));
        }

        if snapshot.on_ac == Some(false) {
            reasons.push("system is on battery".to_string());
        }

        if let Some(pct) = snapshot.battery_pct {
            if pct <= self.thresholds.low_battery_pct {
                reasons.push(format!("battery is low: {pct}%"));
            }
        }

        if let Some(temp) = snapshot.thermal_c() {
            if temp >= self.thresholds.critical_temp_c {
                reasons.push(format!("critical thermal pressure: {temp:.1}C"));
            } else if temp >= self.thresholds.hot_temp_c {
                reasons.push(format!("high thermal pressure: {temp:.1}C"));
            }
        }

        if let Some(cpu) = snapshot.cpu_pressure {
            if cpu.avg10 >= self.thresholds.cpu_pressure_perf_avg10 {
                reasons.push(format!("CPU pressure avg10 is {:.2}", cpu.avg10));
            }
        }

        if let Some(memory) = snapshot.memory_pressure {
            if memory.avg10 >= self.thresholds.memory_pressure_protect_avg10 {
                reasons.push(format!("memory pressure avg10 is {:.2}", memory.avg10));
                actions.push(Action::systemd_set_property(
                    "user.slice".to_string(),
                    vec!["MemoryLow=256M".to_string()],
                    "protect active user sessions from reclaim pressure".to_string(),
                ));
                actions.push(Action::systemd_set_property(
                    "background.slice".to_string(),
                    vec![
                        "CPUWeight=50".to_string(),
                        "IOWeight=50".to_string(),
                        "MemoryHigh=75%".to_string(),
                    ],
                    "throttle background work during memory pressure".to_string(),
                ));
            }
        }

        if let Some(io) = snapshot.io_pressure {
            if io.avg10 >= self.thresholds.io_pressure_throttle_avg10 {
                reasons.push(format!("I/O pressure avg10 is {:.2}", io.avg10));
                actions.push(Action::systemd_set_property(
                    "background.slice".to_string(),
                    vec!["IOWeight=25".to_string()],
                    "reduce background I/O interference".to_string(),
                ));
            }
        }

        let mode_config = match effective_mode {
            Mode::Battery => &self.modes.battery,
            Mode::Balanced => &self.modes.balanced,
            Mode::Performance => &self.modes.performance,
            Mode::Realtime => &self.modes.realtime,
            Mode::Auto => unreachable!("auto is resolved before action planning"),
        };

        actions.push(Action::cpu_epp(
            mode_config.cpu_epp.clone(),
            match effective_mode {
                Mode::Battery => "prefer battery life through CPU energy preference".to_string(),
                Mode::Balanced => {
                    "keep foreground responsiveness without full turbo bias".to_string()
                }
                Mode::Performance => {
                    "reduce CPU wakeup and ramp latency for sustained load".to_string()
                }
                Mode::Realtime => "minimize latency for realtime mode".to_string(),
                _ => "".to_string(),
            },
        ));

        actions.push(Action::platform_profile(
            mode_config.platform_profile.clone(),
            match effective_mode {
                Mode::Battery => "request low-power platform profile".to_string(),
                Mode::Balanced => "request balanced platform profile".to_string(),
                Mode::Performance => "request performance platform profile".to_string(),
                Mode::Realtime => "avoid firmware power-save latency in realtime mode".to_string(),
                _ => "".to_string(),
            },
        ));

        let mut bg_properties = Vec::new();
        if let Some(w) = mode_config.background_cpu_weight {
            bg_properties.push(format!("CPUWeight={w}"));
        }
        if let Some(w) = mode_config.background_io_weight {
            bg_properties.push(format!("IOWeight={w}"));
        }
        if !bg_properties.is_empty() {
            actions.push(Action::systemd_set_property(
                "background.slice".to_string(),
                bg_properties,
                "deprioritize background services on battery".to_string(),
            ));
        }

        let mut user_properties = Vec::new();
        if let Some(w) = mode_config.user_cpu_weight {
            user_properties.push(format!("CPUWeight={w}"));
        }
        if let Some(w) = mode_config.user_io_weight {
            user_properties.push(format!("IOWeight={w}"));
        }
        if !user_properties.is_empty() {
            actions.push(Action::systemd_set_property(
                "user.slice".to_string(),
                user_properties,
                match effective_mode {
                    Mode::Balanced => "favor interactive user sessions".to_string(),
                    Mode::Performance => "boost foreground user work".to_string(),
                    Mode::Realtime => "prioritize controlled realtime user workload".to_string(),
                    _ => "".to_string(),
                },
            ));
        }

        if self.memory.high_swappiness_requires_zram && !snapshot.zram_swap_active {
            reasons.push("vm.* actuation skipped: zram swap is not active".to_string());
        } else {
            // vm.swappiness
            if let Some(swappiness) = mode_config.vm_swappiness {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/swappiness"),
                    swappiness.to_string(),
                    "adjust swappiness for current mode".to_string(),
                ));
            }

            // vm.dirty_background_bytes
            if let Some(bytes) = mode_config.vm_dirty_background_bytes {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/dirty_background_bytes"),
                    bytes.to_string(),
                    "adjust dirty background bytes for current mode".to_string(),
                ));
            }

            // vm.dirty_bytes
            if let Some(bytes) = mode_config.vm_dirty_bytes {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/dirty_bytes"),
                    bytes.to_string(),
                    "adjust dirty bytes for current mode".to_string(),
                ));
            }
        }

        if snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
        {
            actions.push(Action::cpu_epp(
                "balance_power".to_string(),
                "override performance bias because thermals are critical".to_string(),
            ));
            actions.push(Action::platform_profile(
                "balanced".to_string(),
                "back off platform profile under critical thermals".to_string(),
            ));
        }

        // PM QoS wakeup latency (CPU)
        let floors = contracts.resolve(workload_class);
        let cpu_wakeup_latency = Some(floors.cpu_wakeup_latency);
        let device_resume_latency = Some(floors.device_resume_latency);

        let reason_cpu = format!(
            "class={}, floor={}us, row=contracts.{}",
            workload_class, floors.cpu_wakeup_latency, workload_class
        );
        actions.push(Action::CpuDmaLatency {
            value: Some(floors.cpu_wakeup_latency as i32),
            reason: reason_cpu,
        });

        // PM QoS resume latency (Per-device)
        for path in &snapshot.pm_qos_device_paths {
            let reason_dev = format!(
                "class={}, floor={}us, row=contracts.{}",
                workload_class, floors.device_resume_latency, workload_class
            );
            actions.push(Action::DeviceResumeLatency {
                path: path.clone(),
                value: Some(floors.device_resume_latency as i32),
                reason: reason_dev,
            });
        }

        // WP-N5: runtime-PM autosuspend. Conservative "battery-idle" trigger
        // (Decision B): only nominate devices when on battery AND the workload
        // class is idle. The actuator gates each device on the N4 allowlist,
        // skips network devices with an active link, preserves wakeup, and
        // journals for revert-on-stop — so nominating broadly here is safe.
        if snapshot.on_ac == Some(false) && workload_class == WorkloadClass::Idle {
            for device_dir in &snapshot.runtime_pm_device_paths {
                actions.push(Action::RuntimePm {
                    device_dir: device_dir.clone(),
                    autosuspend_delay_ms:
                        crate::actuators::runtime_pm::DEFAULT_AUTOSUSPEND_DELAY_MS,
                    reason: format!(
                        "battery-idle runtime PM (class={workload_class}, allowlist-gated)"
                    ),
                });
            }

            // WP-N6 PCIe ASPM: enable L1 substates on battery-idle. Allowlist-gated
            // (domain pci_aspm); CNVi devices are skipped in the actuator.
            for device_dir in &snapshot.pcie_aspm_device_paths {
                actions.push(Action::PcieAspm {
                    device_dir: device_dir.clone(),
                    enable: true,
                    reason: format!(
                        "battery-idle PCIe ASPM (class={workload_class}, allowlist-gated)"
                    ),
                });
            }

            // WP-N6 SATA ALPM: med_power_with_dipm on battery-idle. Allowlist-gated
            // (domain sata_alpm via the host's backing PCI controller).
            for host_dir in &snapshot.sata_alpm_host_paths {
                actions.push(Action::SataAlpm {
                    host_dir: host_dir.clone(),
                    policy: crate::actuators::storage::DEFAULT_ALPM_POLICY.to_string(),
                    reason: format!(
                        "battery-idle SATA ALPM (class={workload_class}, allowlist-gated)"
                    ),
                });
            }
        }

        if reasons.is_empty() {
            reasons.push("default adaptive policy".to_string());
        }

        Decision {
            mode: effective_mode,
            reasons,
            actions,
            workload_class,
            workload_reason,
            cpu_wakeup_latency,
            device_resume_latency,
        }
    }

    pub(crate) fn is_critical_thermal(&self, snapshot: &Snapshot) -> bool {
        snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
    }

    pub(crate) fn auto_mode(&self, snapshot: &Snapshot) -> Mode {
        if self.is_critical_thermal(snapshot) {
            return Mode::Balanced;
        }

        if snapshot.on_ac == Some(false) {
            if snapshot
                .battery_pct
                .is_some_and(|pct| pct <= self.thresholds.low_battery_pct)
            {
                return Mode::Battery;
            }

            if snapshot
                .cpu_pressure
                .is_some_and(|p| p.avg10 >= self.thresholds.cpu_pressure_perf_avg10)
            {
                return Mode::Balanced;
            }

            return Mode::Battery;
        }

        if snapshot
            .cpu_pressure
            .is_some_and(|p| p.avg10 >= self.thresholds.cpu_pressure_perf_avg10)
        {
            return Mode::Performance;
        }

        Mode::Balanced
    }
}
