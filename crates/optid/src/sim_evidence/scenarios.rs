//! The experiment matrix: the simulated machine, the arms, the environment
//! trajectories, and the injected faults.
//!
//! An *arm* is a configuration of optid. A *scenario* is a modelled situation
//! the machine is in. Every arm is run against every scenario, so a domain
//! cannot hide inside a combined result.

use std::collections::BTreeMap;

use serde::Serialize;

use super::machine::{DeviceSpec, MachineSpec, SataHostSpec, SimFault, SimMachine};
use super::model::{EnvState, WorkloadProfile};

pub(crate) const KERNEL_DOMAINS: [&str; 9] = [
    "cpu_epp",
    "platform_profile",
    "vm_sysctl",
    "cpu_dma_latency",
    "device_resume_latency",
    "runtime_pm",
    "pci_aspm",
    "sata_alpm",
    "backlight",
];

/// The domain that actuates through `systemctl set-property`. It is held off in
/// every arm: driving it would mean executing a real system-service command,
/// which this harness must never do. It is reported as unsupported-in-
/// simulation rather than as a passing test.
pub(crate) const SYSTEMD_DOMAIN: &str = "cgroup_reweight";

/// The standard simulated laptop. One machine shape is used across the whole
/// matrix so that arm-to-arm differences come only from optid.
pub(crate) fn machine_spec() -> MachineSpec {
    MachineSpec {
        name: "rush-sim-laptop-a".to_string(),
        cpus: 4,
        epp_choices: vec![
            "performance".to_string(),
            "balance_performance".to_string(),
            "balance_power".to_string(),
            "power".to_string(),
        ],
        platform_profiles: vec![
            "performance".to_string(),
            "balanced".to_string(),
            "low-power".to_string(),
        ],
        devices: vec![
            // NVMe SSD: the depth-enabler device the matrix cares most about.
            DeviceSpec {
                id: "0000:01:00.0".to_string(),
                bus: "pci",
                modalias: "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02".to_string(),
                class: "0x010802".to_string(),
                pm_qos: true,
                runtime_pm: true,
                aspm: true,
                carrier_up: None,
                inert_controls: Vec::new(),
                readonly_controls: Vec::new(),
            },
            // Intel CNVi radio: PCIe ASPM is firmware-managed, and the seeded
            // allowlist denies it outright. Two independent refusals.
            DeviceSpec {
                id: "0000:00:14.3".to_string(),
                bus: "pci",
                modalias: "pci:v00008086p00002723sv00008086sd00000084bc02sc80i00".to_string(),
                class: "0x028000".to_string(),
                pm_qos: true,
                runtime_pm: true,
                aspm: true,
                carrier_up: None,
                inert_controls: Vec::new(),
                readonly_controls: Vec::new(),
            },
            // Wired NIC with the link up: runtime PM must be skipped.
            DeviceSpec {
                id: "0000:00:1f.6".to_string(),
                bus: "pci",
                modalias: "pci:v00008086p000015FBsv000017AAsd00002259bc02sc00i00".to_string(),
                class: "0x020000".to_string(),
                pm_qos: true,
                runtime_pm: true,
                aspm: true,
                carrier_up: Some(true),
                inert_controls: Vec::new(),
                readonly_controls: Vec::new(),
            },
            // Card reader whose `link/l1_aspm` is inert: the write succeeds and
            // the machine ignores it. The evidence layer must call this
            // unsupported, never a passing actuation.
            DeviceSpec {
                id: "0000:02:00.0".to_string(),
                bus: "pci",
                modalias: "pci:v000010ECp0000522Asv000017AAsd000022C1bc08sc05i00".to_string(),
                class: "0x080501".to_string(),
                pm_qos: true,
                runtime_pm: true,
                aspm: true,
                carrier_up: None,
                inert_controls: vec!["l1_aspm".to_string()],
                readonly_controls: Vec::new(),
            },
            // Root port whose runtime-PM control the kernel exposes read-only.
            DeviceSpec {
                id: "0000:00:1c.0".to_string(),
                bus: "pci",
                modalias: "pci:v00008086p00009A2Fsv000017AAsd000022C2bc06sc04i00".to_string(),
                class: "0x060400".to_string(),
                pm_qos: false,
                runtime_pm: true,
                aspm: false,
                carrier_up: None,
                inert_controls: Vec::new(),
                readonly_controls: vec!["control".to_string()],
            },
            // USB HID receiver.
            DeviceSpec {
                id: "1-4".to_string(),
                bus: "usb",
                modalias: "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00".to_string(),
                class: "0x030000".to_string(),
                pm_qos: true,
                runtime_pm: true,
                aspm: false,
                carrier_up: None,
                inert_controls: Vec::new(),
                readonly_controls: Vec::new(),
            },
        ],
        sata: vec![SataHostSpec {
            host: "host0".to_string(),
            controller: "0000:00:17.0".to_string(),
            modalias: "pci:v00008086p00009D03sv000017AAsd0000222Ebc01sc06i01".to_string(),
        }],
        backlight_device: "intel_backlight".to_string(),
        backlight_gpu_modalias: "pci:v00008086p00009A49sv000017AAsd000022C0bc03sc00i00".to_string(),
        backlight_max: 960,
        zram_swap: true,
    }
}

/// An administrator override that verifies the simulated hardware. This is the
/// production promotion path (`/etc/optid/allowlist.d`), not a test backdoor:
/// without it the shipped seeded baseline denies every device-depth domain
/// because no seeded entry is `verified`.
pub(crate) fn simulation_allowlist_override() -> String {
    let mut out = String::from(
        "# Simulated-hardware allowlist override.\n\
         # Every entry describes MODELLED hardware inside the simulation root.\n\
         # It is not evidence about any physical device.\n\n",
    );
    let entries: [(&str, &str, u64); 9] = [
        (
            "runtime_pm",
            "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02",
            2500,
        ),
        (
            "pci_aspm",
            "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02",
            60,
        ),
        (
            "runtime_pm",
            "pci:v000010ECp0000522Asv000017AAsd000022C1bc08sc05i00",
            4000,
        ),
        (
            "pci_aspm",
            "pci:v000010ECp0000522Asv000017AAsd000022C1bc08sc05i00",
            60,
        ),
        (
            "runtime_pm",
            "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00",
            900,
        ),
        (
            "runtime_pm",
            "pci:v00008086p000015FBsv000017AAsd00002259bc02sc00i00",
            1800,
        ),
        (
            "runtime_pm",
            "pci:v00008086p00009A2Fsv000017AAsd000022C2bc06sc04i00",
            500,
        ),
        (
            "sata_alpm",
            "pci:v00008086p00009D03sv000017AAsd0000222Ebc01sc06i01",
            1500,
        ),
        (
            "backlight",
            "pci:v00008086p00009A49sv000017AAsd000022C0bc03sc00i00",
            0,
        ),
    ];
    for (domain, hwid, exit_latency_us) in entries {
        out.push_str("[[entry]]\n");
        out.push_str(&format!("domain = \"{domain}\"\n"));
        out.push_str(&format!("hwid = \"{hwid}\"\n"));
        out.push_str("action = \"allow\"\n");
        out.push_str("verified = true\n");
        out.push_str(&format!("exit_latency_us = {exit_latency_us}\n"));
        out.push_str(
            "tested_on = \"simulated machine rush-sim-laptop-a (modelled, not physical)\"\n",
        );
        out.push_str(
            "reason = \"deterministic simulation fixture; carries no physical-hardware claim\"\n\n",
        );
    }
    out
}

/// How an arm configures optid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArmKind {
    /// optid never runs. The machine keeps its power-on defaults.
    DaemonAbsent,
    /// The real daemon runs with the given per-domain modes.
    Daemon,
}

/// A deliberately harmful mode table, used as a positive control: if the
/// evidence system cannot see this as harmful, it cannot see harm at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PolicyFlavour {
    Curated,
    Harmful,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Arm {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) kind: ArmKind,
    pub(crate) domains: BTreeMap<String, String>,
    pub(crate) allowlist_override: bool,
    pub(crate) policy: PolicyFlavour,
    /// Arms that are controls rather than measurements.
    pub(crate) control: Option<String>,
}

fn all_domains(mode: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for domain in KERNEL_DOMAINS {
        map.insert(domain.to_string(), mode.to_string());
    }
    map.insert(SYSTEMD_DOMAIN.to_string(), "off".to_string());
    map
}

pub(crate) fn arms() -> Vec<Arm> {
    let mut arms = vec![
        Arm {
            id: "off_absent".to_string(),
            description: "Baseline: optid is not running at all. The machine keeps its power-on \
                          control values for the whole trajectory."
                .to_string(),
            kind: ArmKind::DaemonAbsent,
            domains: all_domains("off"),
            allowlist_override: false,
            policy: PolicyFlavour::Curated,
            control: None,
        },
        Arm {
            id: "off_all_domains".to_string(),
            description: "No-change control: the real daemon runs with every domain off. Any \
                          control value that moves here is a defect in the off path."
                .to_string(),
            kind: ArmKind::Daemon,
            domains: all_domains("off"),
            allowlist_override: true,
            policy: PolicyFlavour::Curated,
            control: Some("no_change".to_string()),
        },
        Arm {
            id: "full_enabled".to_string(),
            description: "Fully enabled: every supported domain may actuate together, with the \
                          simulated hardware verified by an administrator allowlist override."
                .to_string(),
            kind: ArmKind::Daemon,
            domains: all_domains("actuate"),
            allowlist_override: true,
            policy: PolicyFlavour::Curated,
            control: None,
        },
        Arm {
            id: "full_stock_allowlist".to_string(),
            description: "Fully enabled with the shipped seeded allowlist and no administrator \
                          override — the configuration a real installation starts in."
                .to_string(),
            kind: ArmKind::Daemon,
            domains: all_domains("actuate"),
            allowlist_override: false,
            policy: PolicyFlavour::Curated,
            control: None,
        },
        Arm {
            id: "full_observe".to_string(),
            description: "Every domain in observe mode: optid computes the same decisions and \
                          suppresses every write."
                .to_string(),
            kind: ArmKind::Daemon,
            domains: all_domains("observe"),
            allowlist_override: true,
            policy: PolicyFlavour::Curated,
            control: Some("no_change".to_string()),
        },
        Arm {
            id: "harmful_control".to_string(),
            description: "Deliberately harmful control: a policy whose mode table asks for the \
                          worst plausible values for the workload in force."
                .to_string(),
            kind: ArmKind::Daemon,
            domains: all_domains("actuate"),
            allowlist_override: true,
            policy: PolicyFlavour::Harmful,
            control: Some("harmful".to_string()),
        },
    ];
    for domain in KERNEL_DOMAINS {
        let mut domains = all_domains("off");
        domains.insert(domain.to_string(), "actuate".to_string());
        arms.push(Arm {
            id: format!("only_{domain}"),
            description: format!(
                "Isolation arm: only the {domain} domain may actuate; every other domain is off."
            ),
            kind: ArmKind::Daemon,
            domains,
            allowlist_override: true,
            policy: PolicyFlavour::Curated,
            control: None,
        });
    }
    arms
}

/// A modelled situation: an environment trajectory plus an offered workload.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Scenario {
    pub(crate) id: String,
    pub(crate) description: String,
    /// Scenarios that exercise the safety matrix rather than the performance
    /// comparison. Their metrics are still validated, but they are not used to
    /// answer "did optid improve the modelled result".
    pub(crate) safety_only: bool,
    pub(crate) cycles: u32,
    pub(crate) step_seconds: u64,
    pub(crate) env: EnvState,
    pub(crate) workload: WorkloadProfile,
    pub(crate) events: BTreeMap<u32, Vec<StepEvent>>,
    pub(crate) faults: Vec<SimFault>,
}

/// A scripted change to the modelled world, applied at the start of a cycle.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum StepEvent {
    SetAc {
        on_ac: bool,
    },
    SetBattery {
        pct: u8,
    },
    SetAmbient {
        celsius: f64,
    },
    ExternalHeat {
        watts: f64,
    },
    SetWorkload {
        workload: WorkloadProfile,
    },
    SetOfferedLoad {
        loadavg_1: f64,
    },
    /// A foreground application arrives or leaves (writes the pin the GameMode
    /// and foreground shims write in production).
    ForegroundPin {
        class: Option<String>,
    },
    /// Device hotplug: the device directory appears or disappears.
    DeviceRemoved {
        device: String,
        bus: String,
    },
    DeviceRestored {
        device: String,
        bus: String,
    },
    /// CPU hotplug.
    CpuOffline {
        cpu: u32,
    },
    CpuOnline {
        cpu: u32,
    },
    /// Rewrite policy.toml under the running daemon.
    ReloadPolicy {
        valid: bool,
    },
}

pub(crate) fn apply_event(machine: &SimMachine, event: &StepEvent) {
    match event {
        StepEvent::SetAc { on_ac } => machine.with_env(|env| env.on_ac = *on_ac),
        StepEvent::SetBattery { pct } => machine.with_env(|env| env.battery_pct = *pct),
        StepEvent::SetAmbient { celsius } => machine.with_env(|env| env.ambient_c = *celsius),
        StepEvent::ExternalHeat { watts } => machine.with_env(|env| env.external_heat_w = *watts),
        StepEvent::SetWorkload { workload } => {
            machine.with_workload(|current| *current = workload.clone());
            machine.with_env(|env| env.loadavg_1 = workload.cpu_demand);
        }
        StepEvent::SetOfferedLoad { loadavg_1 } => {
            machine.with_env(|env| env.loadavg_1 = *loadavg_1)
        }
        StepEvent::ForegroundPin { class } => machine.set_class_pin(class.as_deref()),
        StepEvent::DeviceRemoved { device, bus } => machine.remove_device(bus, device),
        StepEvent::DeviceRestored { device, bus } => machine.restore_device(bus, device),
        StepEvent::CpuOffline { cpu } => machine.set_cpu_online(*cpu, false),
        StepEvent::CpuOnline { cpu } => machine.set_cpu_online(*cpu, true),
        StepEvent::ReloadPolicy { valid } => machine.reload_policy(*valid),
    }
}

fn env(on_ac: bool, battery: u8, ambient: f64, die: f64, load: f64) -> EnvState {
    EnvState {
        on_ac,
        battery_pct: battery,
        ambient_c: ambient,
        die_temp_c: die,
        skin_temp_c: ambient + 0.35 * (die - ambient),
        loadavg_1: load,
        cpu_pressure: 0.0,
        memory_pressure: 0.0,
        io_pressure: 0.0,
        external_heat_w: 0.0,
        memory_ratio_zram: true,
    }
}

/// Offered-demand shorthand: `(cpu demand, foreground share, CPU wakeups/s,
/// storage ops/s, device wakeups/s, working-set ratio, service µs)`.
type Demand = (f64, f64, f64, f64, f64, f64, f64);

fn workload(id: &str, demand: Demand) -> WorkloadProfile {
    let (
        cpu_demand,
        foreground_share,
        wake_rate_hz,
        io_rate_hz,
        device_wake_rate_hz,
        memory_ratio,
        service_us,
    ) = demand;
    WorkloadProfile {
        id: id.to_string(),
        cpu_demand,
        foreground_share,
        wake_rate_hz,
        io_rate_hz,
        device_wake_rate_hz,
        memory_ratio,
        service_us,
    }
}

pub(crate) fn scenarios() -> Vec<Scenario> {
    let mut list = vec![Scenario {
        id: "idle_battery".to_string(),
        description: "Screen on, nothing running, on battery. The classic case for depth."
            .to_string(),
        safety_only: false,
        cycles: 8,
        step_seconds: 2,
        env: env(false, 82, 24.0, 38.0, 0.02),
        workload: workload("idle", (0.02, 0.9, 5.0, 2.0, 1.0, 0.3, 800.0)),
        events: BTreeMap::new(),
        faults: Vec::new(),
    }];

    list.push(Scenario {
        id: "interactive_ac".to_string(),
        description: "Desktop use on AC: editing, browsing, moderate wakeups.".to_string(),
        safety_only: false,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 46.0, 1.2),
        workload: workload("interactive", (1.2, 0.75, 320.0, 220.0, 60.0, 0.6, 1500.0)),
        events: BTreeMap::new(),
        faults: Vec::new(),
    });

    list.push(Scenario {
        id: "latency_critical_ac".to_string(),
        description: "Latency-critical foreground work on AC with a high wakeup rate.".to_string(),
        safety_only: false,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 52.0, 2.4),
        workload: workload(
            "latency-critical",
            (2.4, 0.92, 900.0, 420.0, 150.0, 0.7, 900.0),
        ),
        events: BTreeMap::new(),
        faults: Vec::new(),
    });

    list.push(Scenario {
        id: "throughput_ac".to_string(),
        description: "Sustained compile-style throughput on AC; the machine is CPU bound."
            .to_string(),
        safety_only: false,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 58.0, 6.0),
        workload: workload("throughput", (6.0, 0.35, 180.0, 900.0, 40.0, 0.8, 2500.0)),
        events: BTreeMap::new(),
        faults: Vec::new(),
    });

    list.push(Scenario {
        id: "memory_pressure_ac".to_string(),
        description: "Working set larger than resident memory; reclaim is on the critical path."
            .to_string(),
        safety_only: false,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 55.0, 3.2),
        workload: workload(
            "memory-pressure",
            (3.2, 0.6, 400.0, 1500.0, 80.0, 1.6, 1800.0),
        ),
        events: BTreeMap::new(),
        faults: Vec::new(),
    });

    list.push(Scenario {
        id: "storage_pressure_ac".to_string(),
        description: "Storage-bound work: high IOPS and heavy writeback.".to_string(),
        safety_only: false,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 50.0, 2.0),
        workload: workload(
            "storage-pressure",
            (2.0, 0.5, 250.0, 3800.0, 300.0, 0.7, 1200.0),
        ),
        events: BTreeMap::new(),
        faults: Vec::new(),
    });

    let background = workload(
        "mixed-background",
        (3.6, 0.30, 260.0, 800.0, 90.0, 0.9, 2000.0),
    );
    let foreground = workload(
        "mixed-foreground",
        (3.6, 0.85, 700.0, 800.0, 90.0, 0.9, 2000.0),
    );
    let mut mixed_events: BTreeMap<u32, Vec<StepEvent>> = BTreeMap::new();
    mixed_events.insert(
        3,
        vec![
            StepEvent::SetWorkload {
                workload: foreground.clone(),
            },
            StepEvent::ForegroundPin {
                class: Some("latency-critical".to_string()),
            },
            StepEvent::SetOfferedLoad { loadavg_1: 3.6 },
        ],
    );
    mixed_events.insert(
        6,
        vec![
            StepEvent::SetWorkload {
                workload: background.clone(),
            },
            StepEvent::ForegroundPin { class: None },
        ],
    );
    list.push(Scenario {
        id: "mixed_foreground_background_battery".to_string(),
        description: "Background work throughout, with a latency-critical foreground application \
                      arriving and leaving (the GameMode / foreground pin path)."
            .to_string(),
        safety_only: false,
        cycles: 9,
        step_seconds: 2,
        env: env(false, 74, 24.0, 50.0, 3.6),
        workload: background,
        events: mixed_events,
        faults: Vec::new(),
    });

    let mut thermal_events: BTreeMap<u32, Vec<StepEvent>> = BTreeMap::new();
    thermal_events.insert(1, vec![StepEvent::ExternalHeat { watts: 34.0 }]);
    thermal_events.insert(2, vec![StepEvent::SetAmbient { celsius: 38.0 }]);
    thermal_events.insert(7, vec![StepEvent::ExternalHeat { watts: 0.0 }]);
    thermal_events.insert(8, vec![StepEvent::SetAmbient { celsius: 24.0 }]);
    list.push(Scenario {
        id: "thermal_rise_and_recovery_ac".to_string(),
        description: "Sustained load with an external heat source that drives the die into the \
                      throttle band, then is removed so the machine recovers."
            .to_string(),
        safety_only: false,
        cycles: 12,
        step_seconds: 4,
        env: env(true, 95, 24.0, 62.0, 5.0),
        workload: workload("thermal", (5.0, 0.5, 300.0, 600.0, 60.0, 0.8, 2000.0)),
        events: thermal_events,
        faults: Vec::new(),
    });

    let mut power_events: BTreeMap<u32, Vec<StepEvent>> = BTreeMap::new();
    power_events.insert(
        4,
        vec![
            StepEvent::SetAc { on_ac: false },
            StepEvent::SetBattery { pct: 64 },
        ],
    );
    // Battery falls below the low-battery threshold, which is a policy input,
    // not something optid controls.
    power_events.insert(6, vec![StepEvent::SetBattery { pct: 18 }]);
    power_events.insert(
        8,
        vec![
            StepEvent::SetAc { on_ac: true },
            StepEvent::SetBattery { pct: 22 },
        ],
    );
    list.push(Scenario {
        id: "ac_to_battery_and_back".to_string(),
        description: "The charger is unplugged mid-run and plugged back in.".to_string(),
        safety_only: false,
        cycles: 12,
        step_seconds: 2,
        env: env(true, 90, 24.0, 48.0, 0.9),
        workload: workload("light", (0.9, 0.8, 260.0, 150.0, 45.0, 0.5, 1400.0)),
        events: power_events,
        faults: Vec::new(),
    });

    // ── Safety scenarios ────────────────────────────────────────────────────
    let mut hotplug_events: BTreeMap<u32, Vec<StepEvent>> = BTreeMap::new();
    hotplug_events.insert(
        3,
        vec![StepEvent::DeviceRemoved {
            device: "0000:02:00.0".to_string(),
            bus: "pci".to_string(),
        }],
    );
    hotplug_events.insert(
        6,
        vec![StepEvent::DeviceRestored {
            device: "0000:02:00.0".to_string(),
            bus: "pci".to_string(),
        }],
    );
    hotplug_events.insert(8, vec![StepEvent::CpuOffline { cpu: 3 }]);
    hotplug_events.insert(11, vec![StepEvent::CpuOnline { cpu: 3 }]);
    list.push(Scenario {
        id: "hotplug_device_and_cpu".to_string(),
        description: "Device and CPU hotplug under a running daemon; the capability topology \
                      changes twice."
            .to_string(),
        safety_only: true,
        cycles: 13,
        step_seconds: 2,
        env: env(false, 70, 24.0, 44.0, 0.3),
        workload: workload("idle", (0.3, 0.8, 40.0, 30.0, 10.0, 0.4, 1000.0)),
        events: hotplug_events,
        faults: Vec::new(),
    });

    let mut reload_events: BTreeMap<u32, Vec<StepEvent>> = BTreeMap::new();
    reload_events.insert(3, vec![StepEvent::ReloadPolicy { valid: false }]);
    reload_events.insert(6, vec![StepEvent::ReloadPolicy { valid: true }]);
    list.push(Scenario {
        id: "config_reload_failure_and_recovery".to_string(),
        description: "policy.toml is replaced with unparseable content under the running daemon, \
                      then repaired."
            .to_string(),
        safety_only: true,
        cycles: 9,
        step_seconds: 2,
        env: env(true, 95, 24.0, 46.0, 1.0),
        workload: workload("interactive", (1.0, 0.8, 300.0, 200.0, 50.0, 0.6, 1500.0)),
        events: BTreeMap::new(),
        faults: Vec::new(),
    });
    if let Some(last) = list.last_mut() {
        last.events = reload_events;
    }

    list.push(Scenario {
        id: "write_failures_and_circuit".to_string(),
        description: "A kernel control starts refusing writes, a second is truncated, and a third \
                      drifts under optid. The circuit breaker is expected to open."
            .to_string(),
        safety_only: true,
        cycles: 10,
        step_seconds: 2,
        env: env(false, 60, 24.0, 44.0, 0.2),
        workload: workload("idle", (0.2, 0.8, 30.0, 20.0, 8.0, 0.4, 1000.0)),
        events: BTreeMap::new(),
        faults: vec![
            SimFault::WriteDenied {
                path: "/sys/firmware/acpi/platform_profile".to_string(),
                at_cycle: 2,
            },
            SimFault::ShortWrite {
                path: "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference"
                    .to_string(),
                at_cycle: 3,
            },
            SimFault::ExternalDrift {
                path: "/proc/sys/vm/swappiness".to_string(),
                at_cycle: 4,
                value: "1".to_string(),
            },
        ],
    });

    list.push(Scenario {
        id: "crash_before_restore".to_string(),
        description: "The daemon dies mid-run without restoring. A later start must find the \
                      journal and hand the machine back."
            .to_string(),
        safety_only: true,
        cycles: 8,
        step_seconds: 2,
        env: env(false, 65, 24.0, 44.0, 0.2),
        workload: workload("idle", (0.2, 0.8, 30.0, 20.0, 8.0, 0.4, 1000.0)),
        events: BTreeMap::new(),
        faults: vec![SimFault::Crash { after_cycle: 3 }],
    });

    list.push(Scenario {
        id: "failed_restoration".to_string(),
        description: "Restoration itself fails at shutdown: the control refuses the handback \
                      write. The workload drives the platform profile away from its power-on \
                      value, so there is a real handback to refuse."
            .to_string(),
        safety_only: true,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 58.0, 6.0),
        workload: workload("throughput", (6.0, 0.35, 180.0, 900.0, 40.0, 0.8, 2500.0)),
        events: BTreeMap::new(),
        faults: vec![SimFault::RestoreDenied {
            path: "/sys/firmware/acpi/platform_profile".to_string(),
        }],
    });

    list.push(Scenario {
        id: "sensor_loss_and_malformed".to_string(),
        description: "The die sensor disappears and a pressure file goes unparseable while the \
                      daemon is running."
            .to_string(),
        safety_only: true,
        cycles: 8,
        step_seconds: 2,
        env: env(true, 95, 24.0, 50.0, 1.5),
        workload: workload("interactive", (1.5, 0.8, 300.0, 200.0, 50.0, 0.6, 1500.0)),
        events: BTreeMap::new(),
        faults: vec![
            SimFault::SensorMissing {
                path: "/sys/class/hwmon/hwmon0/temp1_input".to_string(),
                at_cycle: 3,
            },
            SimFault::SensorMalformed {
                path: "/proc/pressure/cpu".to_string(),
                at_cycle: 5,
                content: "not a pressure file\n".to_string(),
            },
        ],
    });

    list
}
