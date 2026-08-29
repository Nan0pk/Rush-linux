//! The simulated machine: a materialised sysfs/procfs tree inside a verified
//! simulation root, plus the `KernelIo` adapter that serves it to the real
//! daemon.
//!
//! Nothing here models optid. It models a *machine*: which control files exist,
//! which of them accept a write, which values a control accepts, which control
//! is inert (accepts a write and never changes), and what the environment does
//! over time. The daemon under test is the unmodified `crate::run` loop, driven
//! through `kernel_io::with_real_kernel_override`.
//!
//! Containment: every path the daemon touches is either already inside the
//! verified simulation root (the state and config directories, which the daemon
//! reaches with plain `std::fs`) or is rewritten into `<run>/machine/<path>`
//! before any syscall. A path that is neither is refused with `EPERM` and
//! recorded as a containment violation, so a host write cannot be silently
//! absorbed. `is_allowlisted_write_path` — the production write authority — is
//! evaluated on the *unrewritten* path, exactly as `RealKernel` does.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;

use crate::actuator::PmqosSink;
use crate::kernel_io::{is_allowlisted_write_path, Clock, EventSource, KernelRead, KernelWrite};

use super::model::{Assumptions, EnvState, StepMetrics, WorkloadProfile};

/// Value a control file holds, and the rules for changing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ControlKind {
    Enum { allowed: Vec<String> },
    Integer { min: i64, max: i64 },
    Text,
}

/// One writable machine control, and how the modelled hardware responds to it.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ControlSpec {
    /// Stable identity used in receipts, independent of the path spelling.
    pub(crate) id: String,
    /// optid domain that owns this control.
    pub(crate) domain: String,
    pub(crate) kind: ControlKind,
    /// `false` models a control the kernel exposes read-only (write ⇒ EACCES).
    pub(crate) writable: bool,
    /// `false` models an *inert* control: the write is accepted, and the value
    /// never changes. Read-back therefore never matches the request, which is
    /// how the evidence layer learns the lever did not become active.
    pub(crate) active: bool,
}

/// A modelled device attached to the simulated machine.
#[derive(Clone, Debug)]
pub(crate) struct DeviceSpec {
    pub(crate) id: String,
    pub(crate) bus: &'static str,
    pub(crate) modalias: String,
    pub(crate) class: String,
    pub(crate) pm_qos: bool,
    pub(crate) runtime_pm: bool,
    pub(crate) aspm: bool,
    /// `Some(true)` models a network device with an active link.
    pub(crate) carrier_up: Option<bool>,
    /// Controls that exist but are inert on this device.
    pub(crate) inert_controls: Vec<String>,
    /// Controls the kernel refuses to write on this device.
    pub(crate) readonly_controls: Vec<String>,
}

/// A modelled SATA host and the PCI controller behind it.
#[derive(Clone, Debug)]
pub(crate) struct SataHostSpec {
    pub(crate) host: String,
    pub(crate) controller: String,
    pub(crate) modalias: String,
}

/// The static shape of the simulated machine.
#[derive(Clone, Debug)]
pub(crate) struct MachineSpec {
    pub(crate) name: String,
    pub(crate) cpus: u32,
    pub(crate) epp_choices: Vec<String>,
    pub(crate) platform_profiles: Vec<String>,
    pub(crate) devices: Vec<DeviceSpec>,
    pub(crate) sata: Vec<SataHostSpec>,
    pub(crate) backlight_device: String,
    pub(crate) backlight_gpu_modalias: String,
    pub(crate) backlight_max: u64,
    pub(crate) zram_swap: bool,
}

/// Deterministic faults injected into the simulated machine.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "fault", rename_all = "snake_case")]
pub(crate) enum SimFault {
    /// Every write to `path` fails with `EACCES` from `at_cycle` onward.
    WriteDenied { path: String, at_cycle: u32 },
    /// One write to `path` is truncated: the control keeps a partial value.
    ShortWrite { path: String, at_cycle: u32 },
    /// A third party changes `path` behind optid's back.
    ExternalDrift {
        path: String,
        at_cycle: u32,
        value: String,
    },
    /// The sensor file `path` disappears from `at_cycle` onward.
    SensorMissing { path: String, at_cycle: u32 },
    /// The sensor file `path` returns unparseable content from `at_cycle`.
    SensorMalformed {
        path: String,
        at_cycle: u32,
        content: String,
    },
    /// The daemon dies without restoring, immediately after `after_cycle`.
    Crash { after_cycle: u32 },
    /// Restoration writes fail, so shutdown cannot hand the machine back.
    RestoreDenied { path: String },
}

/// Which part of a run a kernel operation belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Startup,
    Cycle,
    Shutdown,
}

/// The outcome the simulated kernel returned for one write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WriteResult {
    Applied,
    Inert,
    Truncated,
    Rejected { reason: String },
    NotAControl,
}

/// What the simulated machine observed about one write, bundled so the record
/// helper stays a single call.
struct Observed {
    control_id: Option<String>,
    domain: Option<String>,
    previous: Option<String>,
    read_back: Option<String>,
    result: WriteResult,
}

/// One observed write attempt against the simulated machine.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriteRecord {
    pub(crate) seq: u64,
    pub(crate) cycle: u32,
    pub(crate) phase: Phase,
    pub(crate) path: String,
    pub(crate) control_id: Option<String>,
    pub(crate) domain: Option<String>,
    pub(crate) previous_value: Option<String>,
    pub(crate) requested_value: String,
    pub(crate) read_back_value: Option<String>,
    pub(crate) result: WriteResult,
}

/// The modelled outcome of one control cycle.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StepSample {
    pub(crate) cycle: u32,
    pub(crate) env: EnvState,
    pub(crate) active: BTreeMap<String, String>,
    pub(crate) metrics: StepMetrics,
}

struct Inner {
    fs_root: PathBuf,
    sim_root: PathBuf,
    state_dir: PathBuf,
    config_path: PathBuf,
    policy_valid: String,
    policy_invalid: String,
    spec: MachineSpec,
    controls: BTreeMap<String, ControlSpec>,
    /// Every control identity ever present on this machine, including those a
    /// hotplug removal took away. Receipts must not lose a domain because the
    /// device went away mid-run.
    domain_registry: BTreeMap<String, String>,
    baseline: BTreeMap<String, String>,
    faults: Vec<SimFault>,
    clock_unix: u64,
    step_seconds: u64,
    cycle: u32,
    cycles_planned: u32,
    phase: Phase,
    crashed: bool,
    seq: u64,
    writes: Vec<WriteRecord>,
    samples: Vec<StepSample>,
    violations: Vec<String>,
    env: EnvState,
    workload: WorkloadProfile,
    assumptions: Assumptions,
    cpu_latency_us: Option<i32>,
    clock_calls: u64,
    runaway_stopped: bool,
    drifted: BTreeSet<String>,
    events: BTreeMap<u32, Vec<super::scenarios::StepEvent>>,
    host_write_attempts: u64,
}

/// The simulated machine. Shared between the injected kernel adapter, the PM
/// QoS sink, and the harness that reads the evidence back out.
pub(crate) struct SimMachine {
    inner: Mutex<Inner>,
}

/// Paths that the simulated kernel serves directly rather than rewriting,
/// because the daemon reaches them with plain `std::fs`.
fn is_inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

impl SimMachine {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        fs_root: PathBuf,
        sim_root: PathBuf,
        state_dir: PathBuf,
        config_path: PathBuf,
        policy_valid: String,
        policy_invalid: String,
        spec: &MachineSpec,
        env: EnvState,
        workload: WorkloadProfile,
        assumptions: Assumptions,
        faults: Vec<SimFault>,
        events: BTreeMap<u32, Vec<super::scenarios::StepEvent>>,
        cycles_planned: u32,
        step_seconds: u64,
        start_unix: u64,
    ) -> io::Result<Arc<Self>> {
        let controls = materialise(&fs_root, spec, &env)?;
        let mut domain_registry = BTreeMap::new();
        for control in controls.values() {
            domain_registry.insert(control.id.clone(), control.domain.clone());
        }
        domain_registry.insert(
            "cpu_dma_latency:cpu".to_string(),
            "cpu_dma_latency".to_string(),
        );
        let mut baseline = BTreeMap::new();
        for path in controls.keys() {
            if let Ok(value) = fs::read_to_string(fs_root.join(path.trim_start_matches('/'))) {
                baseline.insert(path.clone(), value.trim().to_string());
            }
        }
        let machine = Arc::new(Self {
            inner: Mutex::new(Inner {
                fs_root,
                sim_root,
                state_dir,
                config_path,
                policy_valid,
                policy_invalid,
                spec: spec.clone(),
                controls,
                domain_registry,
                baseline,
                faults,
                clock_unix: start_unix,
                step_seconds,
                cycle: 0,
                cycles_planned,
                phase: Phase::Startup,
                crashed: false,
                seq: 0,
                writes: Vec::new(),
                samples: Vec::new(),
                violations: Vec::new(),
                env,
                workload,
                assumptions,
                cpu_latency_us: None,
                clock_calls: 0,
                runaway_stopped: false,
                drifted: BTreeSet::new(),
                events,
                host_write_attempts: 0,
            }),
        });
        machine.apply_events_for_cycle(0);
        Ok(machine)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("simulated machine mutex poisoned")
    }

    /// Rewrite a logical path into the simulated machine tree. Returns `None`
    /// for a path that is neither inside the verified root nor a modelled
    /// machine path.
    fn resolve(inner: &Inner, path: &Path) -> Option<PathBuf> {
        let path = normalise(path);
        if is_inside(&path, &inner.sim_root) {
            return Some(path);
        }
        if path.is_absolute() {
            let relative = path.strip_prefix("/").ok()?;
            return Some(inner.fs_root.join(relative));
        }
        None
    }

    fn logical(inner: &Inner, path: &Path) -> String {
        let path = normalise(path);
        if let Ok(relative) = path.strip_prefix(&inner.fs_root) {
            return format!("/{}", relative.display());
        }
        path.display().to_string()
    }

    /// Logical, symlink-resolved key used to look a control up. `scsi_host`
    /// entries are symlink farms, so the same control has two spellings.
    fn control_key(inner: &Inner, path: &Path) -> String {
        let Some(real) = Self::resolve(inner, path) else {
            return path.display().to_string();
        };
        match fs::canonicalize(&real) {
            Ok(canonical) => Self::logical(inner, &canonical),
            Err(_) => Self::logical(inner, &normalise(path)),
        }
    }

    pub(crate) fn control_values(&self) -> BTreeMap<String, String> {
        let inner = self.lock();
        let mut out = BTreeMap::new();
        for (path, spec) in &inner.controls {
            let value = fs::read_to_string(inner.fs_root.join(path.trim_start_matches('/')))
                .map(|value| value.trim().to_string())
                .unwrap_or_default();
            out.insert(spec.id.clone(), value);
        }
        if let Some(value) = inner.cpu_latency_us {
            out.insert("cpu_dma_latency:cpu".to_string(), value.to_string());
        } else {
            out.insert(
                "cpu_dma_latency:cpu".to_string(),
                "unconstrained".to_string(),
            );
        }
        out
    }

    pub(crate) fn controls(&self) -> BTreeMap<String, ControlSpec> {
        self.lock().controls.clone()
    }

    /// Domain for every control identity the machine has ever exposed.
    pub(crate) fn control_domains(&self) -> BTreeMap<String, String> {
        self.lock().domain_registry.clone()
    }

    pub(crate) fn baseline_values(&self) -> BTreeMap<String, String> {
        let inner = self.lock();
        let mut out = BTreeMap::new();
        for (path, spec) in &inner.controls {
            out.insert(
                spec.id.clone(),
                inner.baseline.get(path).cloned().unwrap_or_default(),
            );
        }
        out.insert(
            "cpu_dma_latency:cpu".to_string(),
            "unconstrained".to_string(),
        );
        out
    }

    pub(crate) fn writes(&self) -> Vec<WriteRecord> {
        self.lock().writes.clone()
    }

    pub(crate) fn samples(&self) -> Vec<StepSample> {
        self.lock().samples.clone()
    }

    pub(crate) fn violations(&self) -> Vec<String> {
        self.lock().violations.clone()
    }

    pub(crate) fn host_write_attempts(&self) -> u64 {
        self.lock().host_write_attempts
    }

    pub(crate) fn cycles_completed(&self) -> u32 {
        self.lock().cycle
    }

    pub(crate) fn enter_shutdown(&self) {
        self.lock().phase = Phase::Shutdown;
    }

    /// Start a fresh daemon lifetime on the same machine, as happens after a
    /// crash or a reboot. Durable state under `/var/lib/optid` is untouched;
    /// the injected crash fault is spent, so the restart can complete.
    pub(crate) fn begin_recovery_phase(&self, cycles: u32) {
        // The world does not stop while the daemon is down. Any scenario event
        // that had not fired yet is applied now, in order, before the restart.
        let pending: Vec<super::scenarios::StepEvent> = {
            let inner = self.lock();
            let reached = inner.cycle;
            inner
                .events
                .iter()
                .filter(|(cycle, _)| **cycle > reached)
                .flat_map(|(_, events)| events.clone())
                .collect()
        };
        for event in pending {
            super::scenarios::apply_event(self, &event);
        }
        let mut inner = self.lock();
        inner.cycle = 0;
        inner.cycles_planned = cycles;
        inner.phase = Phase::Startup;
        inner.crashed = true;
        inner
            .faults
            .retain(|fault| !matches!(fault, SimFault::Crash { .. }));
        inner.events.clear();
        drop(inner);
        self.refresh_sensors();
    }

    /// Model the kernel releasing a process's CPU PM QoS request when its
    /// descriptor on `/dev/cpu_dma_latency` closes, which is what happens when
    /// optid exits for any reason, crash included.
    pub(crate) fn release_cpu_pm_qos(&self) {
        let mut inner = self.lock();
        if inner.cpu_latency_us.is_none() {
            return;
        }
        let previous = inner
            .cpu_latency_us
            .map(|value| value.to_string())
            .unwrap_or_default();
        inner.cpu_latency_us = None;
        inner.phase = Phase::Shutdown;
        Self::record_write(
            &mut inner,
            Path::new("/dev/cpu_dma_latency"),
            "unconstrained",
            Observed {
                control_id: Some("cpu_dma_latency:cpu".to_string()),
                domain: Some("cpu_dma_latency".to_string()),
                previous: Some(previous),
                read_back: Some("unconstrained".to_string()),
                result: WriteResult::Applied,
            },
        );
    }

    /// Control identities a third party changed behind optid's back.
    pub(crate) fn drifted_controls(&self) -> BTreeSet<String> {
        self.lock().drifted.clone()
    }

    /// Advance the environment one step and record the modelled outcome of the
    /// machine state that is active right now. Called at the end of every
    /// control cycle, and directly by the daemon-absent baseline arm.
    pub(crate) fn step_cycle(&self) {
        let (cycle, reached_plan) = {
            let mut inner = self.lock();
            if inner.phase == Phase::Startup {
                inner.phase = Phase::Cycle;
            }
            inner.cycle += 1;
            (inner.cycle, inner.cycle >= inner.cycles_planned)
        };
        let active = self.control_values();
        {
            let mut inner = self.lock();
            let env = inner.env.clone();
            let step_seconds = inner.step_seconds as f64;
            let metrics = super::model::evaluate_step(
                &active,
                &env,
                &inner.workload,
                &inner.assumptions,
                step_seconds,
            );
            inner.samples.push(StepSample {
                cycle,
                env: env.clone(),
                active: active.clone(),
                metrics: metrics.clone(),
            });
            inner.env = super::model::advance_env(&env, &metrics, &inner.assumptions, step_seconds);
            inner.clock_unix += inner.step_seconds;
        }
        self.apply_events_for_cycle(cycle);
        self.apply_drift();
        self.refresh_sensors();
        let _ = reached_plan;
    }

    pub(crate) fn plan_reached(&self) -> bool {
        let inner = self.lock();
        inner.cycle >= inner.cycles_planned
    }

    fn apply_events_for_cycle(&self, cycle: u32) {
        let events = {
            let inner = self.lock();
            inner.events.get(&cycle).cloned().unwrap_or_default()
        };
        for event in events {
            super::scenarios::apply_event(self, &event);
        }
        self.refresh_sensors();
    }

    /// Write or clear the global workload-class pin. In production the
    /// GameMode and foreground shims write exactly this file; the harness
    /// writes it directly so the pin path is exercised without a D-Bus client.
    pub(crate) fn set_class_pin(&self, class: Option<&str>) {
        let inner = self.lock();
        let pin = inner.state_dir.join("workload_class_pin");
        drop(inner);
        match class {
            Some(class) => {
                let _ = fs::create_dir_all(pin.parent().unwrap_or(Path::new("/")));
                let _ = fs::write(&pin, class);
            }
            None => {
                let _ = fs::remove_file(&pin);
            }
        }
    }

    /// Device hotplug: remove the device directory from the simulated machine.
    pub(crate) fn remove_device(&self, bus: &str, device: &str) {
        let mut inner = self.lock();
        let base = format!("/sys/bus/{bus}/devices/{device}");
        let target = inner.fs_root.join(base.trim_start_matches('/'));
        let _ = fs::remove_dir_all(&target);
        inner.controls.retain(|path, _| !path.starts_with(&base));
    }

    /// Device hotplug: bring the device back with its power-on defaults.
    pub(crate) fn restore_device(&self, bus: &str, device: &str) {
        let (fs_root, spec) = {
            let inner = self.lock();
            (inner.fs_root.clone(), inner.spec.clone())
        };
        let Some(device_spec) = spec
            .devices
            .iter()
            .find(|candidate| candidate.id == device && candidate.bus == bus)
            .cloned()
        else {
            return;
        };
        let mut single = spec.clone();
        single.devices = vec![device_spec];
        single.sata = Vec::new();
        let env = self.env();
        if let Ok(controls) = materialise(&fs_root, &single, &env) {
            let mut inner = self.lock();
            let base = format!("/sys/bus/{bus}/devices/{device}");
            for (path, control) in controls {
                if path.starts_with(&base) {
                    inner
                        .baseline
                        .entry(path.clone())
                        .or_insert_with(|| match &control.kind {
                            ControlKind::Enum { allowed } => {
                                allowed.first().cloned().unwrap_or_default()
                            }
                            _ => String::new(),
                        });
                    inner
                        .domain_registry
                        .insert(control.id.clone(), control.domain.clone());
                    inner.controls.insert(path, control);
                }
            }
        }
    }

    /// CPU hotplug.
    pub(crate) fn set_cpu_online(&self, cpu: u32, online: bool) {
        let (fs_root, spec) = {
            let inner = self.lock();
            (inner.fs_root.clone(), inner.spec.clone())
        };
        let base = format!("/sys/devices/system/cpu/cpu{cpu}");
        let target = fs_root.join(base.trim_start_matches('/'));
        if online {
            let path = format!("{base}/cpufreq/energy_performance_preference");
            let _ = write_file(
                &fs_root,
                &format!("{base}/cpufreq/energy_performance_available_preferences"),
                &format!(
                    "{}
",
                    spec.epp_choices.join(" ")
                ),
            );
            let _ = write_file(
                &fs_root,
                &path,
                "balance_performance
",
            );
            let mut inner = self.lock();
            inner
                .baseline
                .entry(path.clone())
                .or_insert_with(|| "balance_performance".to_string());
            inner
                .domain_registry
                .insert(format!("cpu_epp:cpu{cpu}"), "cpu_epp".to_string());
            inner.controls.insert(
                path,
                ControlSpec {
                    id: format!("cpu_epp:cpu{cpu}"),
                    domain: "cpu_epp".to_string(),
                    kind: ControlKind::Enum {
                        allowed: spec.epp_choices.clone(),
                    },
                    writable: true,
                    active: true,
                },
            );
        } else {
            let _ = fs::remove_dir_all(&target);
            let mut inner = self.lock();
            inner.controls.retain(|path, _| !path.starts_with(&base));
        }
    }

    /// Replace policy.toml under the running daemon. `Policy::load` runs on
    /// every cycle, so this is the real configuration-reload path.
    pub(crate) fn reload_policy(&self, valid: bool) {
        let inner = self.lock();
        let path = inner.config_path.clone();
        let body = if valid {
            inner.policy_valid.clone()
        } else {
            inner.policy_invalid.clone()
        };
        drop(inner);
        let _ = fs::write(path, body);
    }

    /// Apply any modelled third-party drift for the cycle just entered.
    fn apply_drift(&self) {
        let (fs_root, faults, cycle) = {
            let inner = self.lock();
            (inner.fs_root.clone(), inner.faults.clone(), inner.cycle)
        };
        for fault in faults {
            if let SimFault::ExternalDrift {
                path,
                at_cycle,
                value,
            } = fault
            {
                if cycle == at_cycle {
                    let target = fs_root.join(path.trim_start_matches('/'));
                    let _ = fs::write(target, format!("{value}\n"));
                    let mut inner = self.lock();
                    if let Some(control) = inner.controls.get(&path).cloned() {
                        inner.drifted.insert(control.id);
                    }
                }
            }
        }
    }

    /// Mutate the modelled environment. Used by scenario events.
    pub(crate) fn with_env(&self, mutate: impl FnOnce(&mut EnvState)) {
        let mut inner = self.lock();
        mutate(&mut inner.env);
    }

    pub(crate) fn with_workload(&self, mutate: impl FnOnce(&mut WorkloadProfile)) {
        let mut inner = self.lock();
        mutate(&mut inner.workload);
    }

    pub(crate) fn env(&self) -> EnvState {
        self.lock().env.clone()
    }

    /// Rewrite the sensor surface so the next snapshot observes the current
    /// modelled environment. Sensor files are machine state, not optid state:
    /// they are written by the model, never by the daemon.
    pub(crate) fn refresh_sensors(&self) {
        let inner = self.lock();
        let root = inner.fs_root.clone();
        let env = inner.env.clone();
        let faults = inner.faults.clone();
        let cycle = inner.cycle;
        drop(inner);
        let hidden = |path: &str| {
            faults.iter().any(|fault| match fault {
                SimFault::SensorMissing { path: p, at_cycle } => p == path && cycle >= *at_cycle,
                _ => false,
            })
        };
        let malformed = |path: &str| -> Option<String> {
            faults.iter().find_map(|fault| match fault {
                SimFault::SensorMalformed {
                    path: p,
                    at_cycle,
                    content,
                } if p == path && cycle >= *at_cycle => Some(content.clone()),
                _ => None,
            })
        };
        let put = |path: &str, value: String| {
            let target = root.join(path.trim_start_matches('/'));
            if hidden(path) {
                let _ = fs::remove_file(&target);
                return;
            }
            let value = malformed(path).unwrap_or(value);
            if let Some(parent) = target.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&target, value);
        };
        put(
            "/proc/loadavg",
            format!(
                "{:.2} {:.2} {:.2} 1/300 1234\n",
                env.loadavg_1, env.loadavg_1, env.loadavg_1
            ),
        );
        for (name, value) in [
            ("cpu", env.cpu_pressure),
            ("memory", env.memory_pressure),
            ("io", env.io_pressure),
        ] {
            let body = if name == "cpu" {
                format!("some avg10={value:.2} avg60={value:.2} avg300={value:.2} total=0\n")
            } else {
                format!(
                    "some avg10={value:.2} avg60={value:.2} avg300={value:.2} total=0\nfull avg10={value:.2} avg60={value:.2} avg300={value:.2} total=0\n"
                )
            };
            put(&format!("/proc/pressure/{name}"), body);
        }
        put(
            "/sys/class/power_supply/ACAD/online",
            format!("{}\n", if env.on_ac { 1 } else { 0 }),
        );
        put(
            "/sys/class/power_supply/BAT0/capacity",
            format!("{}\n", env.battery_pct),
        );
        let millic = (env.die_temp_c * 1000.0).round() as i64;
        put("/sys/class/hwmon/hwmon0/temp1_input", format!("{millic}\n"));
        put(
            "/sys/class/thermal/thermal_zone0/temp",
            format!("{millic}\n"),
        );
        let skin = (env.skin_temp_c * 1000.0).round() as i64;
        put("/sys/class/hwmon/hwmon1/temp1_input", format!("{skin}\n"));
    }

    fn record_write(inner: &mut Inner, path: &Path, requested: &str, observed: Observed) {
        let Observed {
            control_id,
            domain,
            previous,
            read_back,
            result,
        } = observed;
        inner.seq += 1;
        let seq = inner.seq;
        let cycle = inner.cycle;
        let phase = inner.phase;
        inner.writes.push(WriteRecord {
            seq,
            cycle,
            phase,
            path: path.display().to_string(),
            control_id,
            domain,
            previous_value: previous,
            requested_value: requested.trim().to_string(),
            read_back_value: read_back,
            result,
        });
    }

    fn fault_for_write(inner: &Inner, key: &str) -> Option<SimFault> {
        let cycle = inner.cycle;
        let phase = inner.phase;
        inner.faults.iter().find_map(|fault| match fault {
            SimFault::WriteDenied { path, at_cycle } if path == key && cycle >= *at_cycle => {
                Some(fault.clone())
            }
            SimFault::ShortWrite { path, at_cycle } if path == key && cycle == *at_cycle => {
                Some(fault.clone())
            }
            SimFault::RestoreDenied { path } if path == key && phase == Phase::Shutdown => {
                Some(fault.clone())
            }
            _ => None,
        })
    }

    fn crash_due(inner: &Inner) -> bool {
        inner.faults.iter().any(|fault| match fault {
            SimFault::Crash { after_cycle } => inner.cycle >= *after_cycle && !inner.crashed,
            _ => false,
        })
    }

    /// Apply a kernel-control write with the modelled device semantics.
    fn control_write(&self, path: &Path, value: &str) -> io::Result<()> {
        let mut inner = self.lock();
        let key = Self::control_key(&inner, path);
        let Some(spec) = inner.controls.get(&key).cloned() else {
            inner.host_write_attempts += 1;
            inner
                .violations
                .push(format!("write to unmodelled machine path {key}"));
            Self::record_write(
                &mut inner,
                path,
                value,
                Observed {
                    control_id: None,
                    domain: None,
                    previous: None,
                    read_back: None,
                    result: WriteResult::NotAControl,
                },
            );
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("simulated machine has no control at {key}"),
            ));
        };
        let target = inner.fs_root.join(key.trim_start_matches('/'));
        let previous = fs::read_to_string(&target)
            .map(|value| value.trim().to_string())
            .ok();
        let requested = value.trim().to_string();

        if let Some(fault) = Self::fault_for_write(&inner, &key) {
            match fault {
                SimFault::WriteDenied { .. } | SimFault::RestoreDenied { .. } => {
                    Self::record_write(
                        &mut inner,
                        path,
                        value,
                        Observed {
                            control_id: Some(spec.id.clone()),
                            domain: Some(spec.domain.clone()),
                            previous: previous.clone(),
                            read_back: previous,
                            result: WriteResult::Rejected {
                                reason: "injected_write_denied".to_string(),
                            },
                        },
                    );
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("injected write denial on {key}"),
                    ));
                }
                SimFault::ShortWrite { .. } => {
                    let truncated: String = requested.chars().take(requested.len() / 2).collect();
                    let _ = fs::write(&target, format!("{truncated}\n"));
                    Self::record_write(
                        &mut inner,
                        path,
                        value,
                        Observed {
                            control_id: Some(spec.id.clone()),
                            domain: Some(spec.domain.clone()),
                            previous,
                            read_back: Some(truncated),
                            result: WriteResult::Truncated,
                        },
                    );
                    return Ok(());
                }
                _ => {}
            }
        }

        if !spec.writable {
            Self::record_write(
                &mut inner,
                path,
                value,
                Observed {
                    control_id: Some(spec.id.clone()),
                    domain: Some(spec.domain.clone()),
                    previous: previous.clone(),
                    read_back: previous,
                    result: WriteResult::Rejected {
                        reason: "read_only_control".to_string(),
                    },
                },
            );
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{key} is read-only on this machine"),
            ));
        }

        let accepted = match &spec.kind {
            ControlKind::Enum { allowed } => allowed.iter().any(|value| value == &requested),
            ControlKind::Integer { min, max } => requested
                .parse::<i64>()
                .map(|value| value >= *min && value <= *max)
                .unwrap_or(false),
            ControlKind::Text => true,
        };
        if !accepted {
            Self::record_write(
                &mut inner,
                path,
                value,
                Observed {
                    control_id: Some(spec.id.clone()),
                    domain: Some(spec.domain.clone()),
                    previous: previous.clone(),
                    read_back: previous,
                    result: WriteResult::Rejected {
                        reason: "value_not_accepted".to_string(),
                    },
                },
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{key} rejects value {requested}"),
            ));
        }

        if !spec.active {
            // An inert lever: the write succeeds and the machine ignores it.
            Self::record_write(
                &mut inner,
                path,
                value,
                Observed {
                    control_id: Some(spec.id.clone()),
                    domain: Some(spec.domain.clone()),
                    previous: previous.clone(),
                    read_back: previous,
                    result: WriteResult::Inert,
                },
            );
            return Ok(());
        }

        fs::write(&target, format!("{requested}\n"))?;
        Self::record_write(
            &mut inner,
            path,
            value,
            Observed {
                control_id: Some(spec.id.clone()),
                domain: Some(spec.domain.clone()),
                previous,
                read_back: Some(requested),
                result: WriteResult::Applied,
            },
        );
        Ok(())
    }

    fn note_cycle_boundary(&self) -> io::Result<()> {
        if Self::crash_due(&self.lock()) {
            let mut inner = self.lock();
            inner.crashed = true;
            return Err(io::Error::other(
                "simulated daemon crash before the cycle envelope was durable",
            ));
        }
        self.step_cycle();
        if self.plan_reached() {
            self.enter_shutdown();
            // SIGTERM is caught by the flag `run()` registers, so the loop
            // leaves through its ordinary clean-shutdown path.
            unsafe { libc::raise(libc::SIGTERM) };
        }
        Ok(())
    }
}

/// The `KernelIo` adapter handed to `with_real_kernel_override`.
pub(crate) struct SimKernel {
    machine: Arc<SimMachine>,
}

impl SimKernel {
    pub(crate) fn new(machine: Arc<SimMachine>) -> Self {
        Self { machine }
    }

    fn resolved(&self, path: &Path) -> io::Result<PathBuf> {
        let inner = self.machine.lock();
        SimMachine::resolve(&inner, path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("path outside the simulation root: {}", path.display()),
            )
        })
    }

    fn refuse_host(&self, path: &Path, operation: &str) -> io::Error {
        let mut inner = self.machine.lock();
        inner.host_write_attempts += 1;
        inner.violations.push(format!(
            "{operation} outside the simulation root: {}",
            path.display()
        ));
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to touch a host path from the simulation",
        )
    }
}

impl KernelRead for SimKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let real = self.resolved(path)?;
        fs::read_to_string(real)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let real = self.resolved(path)?;
        let mut entries: Vec<PathBuf> = fs::read_dir(&real)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        entries.sort();
        let inner = self.machine.lock();
        Ok(entries
            .into_iter()
            .map(|entry| PathBuf::from(SimMachine::logical(&inner, &entry)))
            .collect())
    }

    fn exists(&self, path: &Path) -> bool {
        match self.resolved(path) {
            Ok(real) => real.exists(),
            Err(_) => false,
        }
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        let real = self.resolved(path)?;
        let target = fs::read_link(real)?;
        Ok(target)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let real = self.resolved(path)?;
        let canonical = fs::canonicalize(real)?;
        let inner = self.machine.lock();
        Ok(PathBuf::from(SimMachine::logical(&inner, &canonical)))
    }
}

impl KernelWrite for SimKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        // The production write authority runs on the unrewritten path first,
        // exactly as `RealKernel::write` does.
        is_allowlisted_write_path(path)?;
        self.machine.control_write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        let real = self
            .resolved(path)
            .map_err(|_| self.refuse_host(path, "state write"))?;
        if let Some(parent) = real.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(real, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let real = self
            .resolved(path)
            .map_err(|_| self.refuse_host(path, "directory creation"))?;
        fs::create_dir_all(real)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let source = self
            .resolved(from)
            .map_err(|_| self.refuse_host(from, "rename source"))?;
        let target = self
            .resolved(to)
            .map_err(|_| self.refuse_host(to, "rename target"))?;
        fs::rename(source, target)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let real = self
            .resolved(path)
            .map_err(|_| self.refuse_host(path, "unlink"))?;
        fs::remove_file(real)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        use std::io::Write;
        let real = self
            .resolved(path)
            .map_err(|_| self.refuse_host(path, "append"))?;
        if let Some(parent) = real.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&real)?;
        file.write_all(text.as_bytes())?;
        drop(file);
        // The control-cycle envelope is the daemon's own "one cycle finished"
        // marker. It is written after actuation and before shutdown restore, so
        // it is the exact point at which the machine state is fully active.
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "control-cycles.jsonl")
        {
            self.machine.note_cycle_boundary()?;
        }
        Ok(())
    }
}

impl Clock for SimKernel {
    fn now_unix(&self) -> u64 {
        let (now, runaway) = {
            let mut inner = self.machine.lock();
            inner.clock_calls += 1;
            // Safety valve. The daemon loop is ended by the SIGTERM the cycle
            // boundary raises; if a cycle ever failed to reach that boundary
            // the loop would spin forever, so a hard bound on clock reads ends
            // the run and records why.
            let budget = (inner.cycles_planned as u64 + 4) * 4_000;
            let runaway = inner.clock_calls > budget && !inner.runaway_stopped;
            if runaway {
                inner.runaway_stopped = true;
                inner.violations.push(format!(
                    "run exceeded the {budget}-clock-read budget without completing its planned \
                     cycles; the harness stopped it"
                ));
            }
            (inner.clock_unix, runaway)
        };
        if runaway {
            unsafe { libc::raise(libc::SIGTERM) };
        }
        now
    }
}

impl EventSource for SimKernel {
    fn wait(&self, _duration: Duration) -> bool {
        false
    }
}

/// PM QoS sink for the simulated machine. `/dev/cpu_dma_latency` is a character
/// device, not a file, so it cannot ride the `KernelIo` seam.
pub(crate) struct SimPmqosSink {
    machine: Arc<SimMachine>,
}

impl SimPmqosSink {
    pub(crate) fn new(machine: Arc<SimMachine>) -> Self {
        Self { machine }
    }
}

impl PmqosSink for SimPmqosSink {
    fn read_cpu_latency(&self) -> io::Result<String> {
        let inner = self.machine.lock();
        Ok(inner
            .cpu_latency_us
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unconstrained".to_string()))
    }

    fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
        let path = Path::new("/dev/cpu_dma_latency");
        let mut inner = self.machine.lock();
        let previous = inner
            .cpu_latency_us
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unconstrained".to_string());
        let requested = value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unconstrained".to_string());
        if let Some(fault) = Self::denied(&inner, path) {
            let _ = fault;
            SimMachine::record_write(
                &mut inner,
                path,
                &requested,
                Observed {
                    control_id: Some("cpu_dma_latency:cpu".to_string()),
                    domain: Some("cpu_dma_latency".to_string()),
                    previous: Some(previous.clone()),
                    read_back: Some(previous),
                    result: WriteResult::Rejected {
                        reason: "injected_write_denied".to_string(),
                    },
                },
            );
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected CPU PM QoS denial",
            ));
        }
        inner.cpu_latency_us = value;
        SimMachine::record_write(
            &mut inner,
            path,
            &requested,
            Observed {
                control_id: Some("cpu_dma_latency:cpu".to_string()),
                domain: Some("cpu_dma_latency".to_string()),
                previous: Some(previous),
                read_back: Some(requested.clone()),
                result: WriteResult::Applied,
            },
        );
        Ok(())
    }

    fn read_device_latency(&self, device_path: &Path) -> io::Result<String> {
        SimKernel::new(Arc::clone(&self.machine)).read_to_string(device_path)
    }

    fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()> {
        SimKernel::new(Arc::clone(&self.machine)).write(device_path, value)
    }
}

impl SimPmqosSink {
    fn denied(inner: &Inner, path: &Path) -> Option<SimFault> {
        let key = path.display().to_string();
        SimMachine::fault_for_write(inner, &key)
    }
}

fn write_file(root: &Path, path: &str, value: &str) -> io::Result<()> {
    let target = root.join(path.trim_start_matches('/'));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, value)
}

/// Build the machine tree on disk and return its control table.
fn materialise(
    root: &Path,
    spec: &MachineSpec,
    env: &EnvState,
) -> io::Result<BTreeMap<String, ControlSpec>> {
    let mut controls: BTreeMap<String, ControlSpec> = BTreeMap::new();
    fs::create_dir_all(root)?;

    write_file(root, "/sys/class/dmi/id/sys_vendor", "HP\n")?;
    write_file(
        root,
        "/sys/class/dmi/id/product_name",
        &format!("{}\n", spec.name),
    )?;
    write_file(
        root,
        "/proc/swaps",
        if spec.zram_swap {
            "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n/dev/zram0                              partition\t8388604\t\t0\t\t100\n"
        } else {
            "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n/dev/nvme0n1p3                          partition\t8388604\t\t0\t\t-2\n"
        },
    )?;

    // CPU energy-performance preference, one control per CPU.
    for cpu in 0..spec.cpus {
        let base = format!("/sys/devices/system/cpu/cpu{cpu}");
        write_file(
            root,
            &format!("{base}/cpufreq/energy_performance_available_preferences"),
            &format!("{}\n", spec.epp_choices.join(" ")),
        )?;
        let path = format!("{base}/cpufreq/energy_performance_preference");
        write_file(root, &path, "balance_performance\n")?;
        controls.insert(
            path,
            ControlSpec {
                id: format!("cpu_epp:cpu{cpu}"),
                domain: "cpu_epp".to_string(),
                kind: ControlKind::Enum {
                    allowed: spec.epp_choices.clone(),
                },
                writable: true,
                active: true,
            },
        );
    }
    fs::create_dir_all(root.join("sys/devices/system/cpu/cpufreq"))?;

    // Firmware platform profile.
    write_file(
        root,
        "/sys/firmware/acpi/platform_profile_choices",
        &format!("{}\n", spec.platform_profiles.join(" ")),
    )?;
    write_file(root, "/sys/firmware/acpi/platform_profile", "balanced\n")?;
    controls.insert(
        "/sys/firmware/acpi/platform_profile".to_string(),
        ControlSpec {
            id: "platform_profile:acpi".to_string(),
            domain: "platform_profile".to_string(),
            kind: ControlKind::Enum {
                allowed: spec.platform_profiles.clone(),
            },
            writable: true,
            active: true,
        },
    );

    // VM sysctls.
    for (path, id, min, max, default) in [
        (
            "/proc/sys/vm/swappiness",
            "vm_sysctl:swappiness",
            0,
            200,
            "60",
        ),
        (
            "/proc/sys/vm/dirty_background_bytes",
            "vm_sysctl:dirty_background_bytes",
            0,
            1 << 34,
            "0",
        ),
        (
            "/proc/sys/vm/dirty_bytes",
            "vm_sysctl:dirty_bytes",
            0,
            1 << 34,
            "0",
        ),
    ] {
        write_file(root, path, &format!("{default}\n"))?;
        controls.insert(
            path.to_string(),
            ControlSpec {
                id: id.to_string(),
                domain: "vm_sysctl".to_string(),
                kind: ControlKind::Integer { min, max },
                writable: true,
                active: true,
            },
        );
    }

    // Devices.
    for device in &spec.devices {
        let base = format!("/sys/bus/{}/devices/{}", device.bus, device.id);
        write_file(
            root,
            &format!("{base}/modalias"),
            &format!("{}\n", device.modalias),
        )?;
        write_file(
            root,
            &format!("{base}/class"),
            &format!("{}\n", device.class),
        )?;
        write_file(root, &format!("{base}/power/wakeup"), "enabled\n")?;
        if let Some(up) = device.carrier_up {
            write_file(
                root,
                &format!("{base}/net/eth0/carrier"),
                if up { "1\n" } else { "0\n" },
            )?;
        }
        if device.pm_qos {
            let path = format!("{base}/power/pm_qos_resume_latency_us");
            write_file(root, &path, "0\n")?;
            controls.insert(
                path,
                ControlSpec {
                    id: format!("device_resume_latency:{}", device.id),
                    domain: "device_resume_latency".to_string(),
                    kind: ControlKind::Text,
                    writable: !device.readonly_controls.iter().any(|c| c == "pm_qos"),
                    active: !device.inert_controls.iter().any(|c| c == "pm_qos"),
                },
            );
        }
        if device.runtime_pm {
            let control = format!("{base}/power/control");
            write_file(root, &control, "on\n")?;
            controls.insert(
                control,
                ControlSpec {
                    id: format!("runtime_pm_control:{}", device.id),
                    domain: "runtime_pm".to_string(),
                    kind: ControlKind::Enum {
                        allowed: vec!["on".to_string(), "auto".to_string()],
                    },
                    writable: !device.readonly_controls.iter().any(|c| c == "control"),
                    active: !device.inert_controls.iter().any(|c| c == "control"),
                },
            );
            let delay = format!("{base}/power/autosuspend_delay_ms");
            write_file(root, &delay, "-1\n")?;
            controls.insert(
                delay,
                ControlSpec {
                    id: format!("runtime_pm_delay:{}", device.id),
                    domain: "runtime_pm".to_string(),
                    kind: ControlKind::Integer {
                        min: -1,
                        max: 3_600_000,
                    },
                    writable: true,
                    active: !device.inert_controls.iter().any(|c| c == "autosuspend"),
                },
            );
        }
        if device.aspm {
            let path = format!("{base}/link/l1_aspm");
            write_file(root, &path, "0\n")?;
            controls.insert(
                path,
                ControlSpec {
                    id: format!("pci_aspm:{}", device.id),
                    domain: "pci_aspm".to_string(),
                    kind: ControlKind::Enum {
                        allowed: vec!["0".to_string(), "1".to_string()],
                    },
                    writable: !device.readonly_controls.iter().any(|c| c == "l1_aspm"),
                    active: !device.inert_controls.iter().any(|c| c == "l1_aspm"),
                },
            );
        }
    }

    // SATA hosts: the control lives on the ATA link under the PCI controller,
    // and `/sys/class/scsi_host/<host>` is the symlink farm the kernel exposes.
    for host in &spec.sata {
        let controller = format!("/sys/devices/pci0000:00/{}", host.controller);
        write_file(
            root,
            &format!("{controller}/modalias"),
            &format!("{}\n", host.modalias),
        )?;
        let real = format!("{controller}/ata1/{}", host.host);
        let path = format!("{real}/link_power_management_policy");
        write_file(root, &path, "max_performance\n")?;
        controls.insert(
            path,
            ControlSpec {
                id: format!("sata_alpm:{}", host.host),
                domain: "sata_alpm".to_string(),
                kind: ControlKind::Enum {
                    allowed: vec![
                        "max_performance".to_string(),
                        "medium_power".to_string(),
                        "med_power_with_dipm".to_string(),
                        "min_power".to_string(),
                    ],
                },
                writable: true,
                active: true,
            },
        );
        let link_dir = root.join("sys/class/scsi_host");
        fs::create_dir_all(&link_dir)?;
        let link = link_dir.join(&host.host);
        if !link.exists() {
            std::os::unix::fs::symlink(
                PathBuf::from("../../devices/pci0000:00")
                    .join(&host.controller)
                    .join("ata1")
                    .join(&host.host),
                &link,
            )?;
        }
    }

    // Backlight.
    let backlight = format!("/sys/class/backlight/{}", spec.backlight_device);
    write_file(
        root,
        &format!("{backlight}/max_brightness"),
        &format!("{}\n", spec.backlight_max),
    )?;
    write_file(
        root,
        &format!("{backlight}/modalias"),
        &format!("{}\n", spec.backlight_gpu_modalias),
    )?;
    let brightness = format!("{backlight}/brightness");
    write_file(root, &brightness, &format!("{}\n", spec.backlight_max))?;
    controls.insert(
        brightness,
        ControlSpec {
            id: format!("backlight:{}", spec.backlight_device),
            domain: "backlight".to_string(),
            kind: ControlKind::Integer {
                min: 0,
                max: spec.backlight_max as i64,
            },
            writable: true,
            active: true,
        },
    );

    // Thermal and power sensors.
    write_file(root, "/sys/class/hwmon/hwmon0/name", "coretemp\n")?;
    write_file(
        root,
        "/sys/class/hwmon/hwmon0/temp1_label",
        "Package id 0\n",
    )?;
    write_file(root, "/sys/class/hwmon/hwmon1/name", "acpitz\n")?;
    write_file(root, "/sys/class/hwmon/hwmon1/temp1_label", "skin\n")?;
    write_file(
        root,
        "/sys/class/thermal/thermal_zone0/type",
        "x86_pkg_temp\n",
    )?;
    write_file(root, "/sys/class/power_supply/ACAD/type", "Mains\n")?;
    write_file(root, "/sys/class/power_supply/BAT0/type", "Battery\n")?;
    let millic = (env.die_temp_c * 1000.0).round() as i64;
    write_file(
        root,
        "/sys/class/hwmon/hwmon0/temp1_input",
        &format!("{millic}\n"),
    )?;
    write_file(
        root,
        "/sys/class/hwmon/hwmon1/temp1_input",
        &format!("{millic}\n"),
    )?;
    write_file(
        root,
        "/sys/class/thermal/thermal_zone0/temp",
        &format!("{millic}\n"),
    )?;
    write_file(
        root,
        "/sys/class/power_supply/ACAD/online",
        if env.on_ac { "1\n" } else { "0\n" },
    )?;
    write_file(
        root,
        "/sys/class/power_supply/BAT0/capacity",
        &format!("{}\n", env.battery_pct),
    )?;
    write_file(root, "/proc/loadavg", "0.00 0.00 0.00 1/300 1234\n")?;
    for name in ["cpu", "memory", "io"] {
        write_file(
            root,
            &format!("/proc/pressure/{name}"),
            "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
        )?;
    }

    Ok(controls)
}
