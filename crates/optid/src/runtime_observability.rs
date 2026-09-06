//! O1 read-only runtime-state observability.
//!
//! The observer reads stable kernel interfaces only. It owns no actuator or
//! hardware-write path. Counter history is explicit so the daemon can report
//! real deltas without turning a reset, wrap, or stale sample into a huge
//! fabricated value.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

// O1 consumes the frozen F2 read seam through the `optid` library target.
// Reaching for the module directly would pull the write and fault paths
// into this read-only reporter, which owns neither.
use optid::{Clock, KernelRead};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuntimeObservabilityMode {
    Off,
    #[default]
    Observe,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeConfig {
    #[serde(default)]
    mode: RuntimeObservabilityMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservabilityConfig {
    #[serde(default)]
    runtime: RuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PolicyFragment {
    #[serde(default)]
    observability: ObservabilityConfig,
}

impl RuntimeObservabilityMode {
    /// Parse only the O1 policy fragment. Unknown top-level policy sections are
    /// intentionally ignored; malformed or unreadable policy falls back to the
    /// documented read-only `observe` default.
    pub(crate) fn from_policy_file(read: &dyn KernelRead, path: &Path) -> Self {
        let Ok(text) = read.read_to_string(path) else {
            return Self::Observe;
        };
        toml::from_str::<PolicyFragment>(&text)
            .map(|fragment| fragment.observability.runtime.mode)
            .unwrap_or(Self::Observe)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ObservationStatus {
    Observed,
    /// The kernel answered, and the answer is that the surface does not apply
    /// to this device — a `power/runtime_status` of `unsupported`, meaning the
    /// driver implements no runtime PM. Nothing is missing and nothing is
    /// wrong, so this is neither `Unsupported` (the interface is absent) nor
    /// `Malformed` (the value is corrupt). Kept distinct so an operator can
    /// tell "this device has no runtime PM" from "optid could not read it".
    NotApplicable,
    #[default]
    Unsupported,
    PermissionDenied,
    Malformed,
    Stale,
    Disabled,
}

impl ObservationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::NotApplicable => "not_applicable",
            Self::Unsupported => "unsupported",
            Self::PermissionDenied => "permission_denied",
            Self::Malformed => "malformed",
            Self::Stale => "stale",
            Self::Disabled => "disabled",
        }
    }

    fn merge(self, other: Self) -> Self {
        fn rank(status: ObservationStatus) -> u8 {
            match status {
                ObservationStatus::Observed => 0,
                // Ranked just above `Observed` so a not-applicable device
                // still surfaces a real fault found on any of its other
                // files, and does not itself mask one.
                ObservationStatus::NotApplicable => 1,
                ObservationStatus::Unsupported => 2,
                ObservationStatus::Stale => 3,
                ObservationStatus::Malformed => 4,
                ObservationStatus::PermissionDenied => 5,
                ObservationStatus::Disabled => 6,
            }
        }
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CounterDelta {
    pub(crate) value: Option<u64>,
    pub(crate) reset_or_wrap: bool,
}

impl CounterDelta {
    fn between(current: Option<u64>, previous: Option<u64>, stale: bool) -> Self {
        if stale {
            return Self::default();
        }
        match (current, previous) {
            (Some(current), Some(previous)) if current >= previous => Self {
                value: Some(current - previous),
                reset_or_wrap: false,
            },
            (Some(_), Some(_)) => Self {
                value: None,
                reset_or_wrap: true,
            },
            _ => Self::default(),
        }
    }

    fn render(self) -> String {
        if self.reset_or_wrap {
            "reset_or_wrap".to_string()
        } else {
            self.value
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string())
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WakeupSourceObservation {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: ObservationStatus,
    pub(crate) event_count: Option<u64>,
    pub(crate) event_delta: CounterDelta,
    pub(crate) wakeup_count: Option<u64>,
    pub(crate) wakeup_delta: CounterDelta,
    pub(crate) total_time_us: Option<u64>,
    pub(crate) total_time_delta_us: CounterDelta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RuntimePmObservation {
    pub(crate) id: String,
    pub(crate) status: ObservationStatus,
    pub(crate) runtime_status: Option<String>,
    pub(crate) control: Option<String>,
    pub(crate) active_time_us: Option<u64>,
    pub(crate) active_time_delta_us: CounterDelta,
    pub(crate) suspended_time_us: Option<u64>,
    pub(crate) suspended_time_delta_us: CounterDelta,
    pub(crate) resume_latency_constraint_us: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CpuIdleObservation {
    pub(crate) cpu: u32,
    pub(crate) state: String,
    pub(crate) status: ObservationStatus,
    pub(crate) time_us: Option<u64>,
    pub(crate) time_delta_us: CounterDelta,
    pub(crate) usage: Option<u64>,
    pub(crate) usage_delta: CounterDelta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PmQosObservation {
    pub(crate) status: ObservationStatus,
    pub(crate) effective_cpu_latency_us: Option<i64>,
    pub(crate) requestor_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StorageObservation {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: ObservationStatus,
    pub(crate) state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BacklightObservation {
    pub(crate) id: String,
    pub(crate) status: ObservationStatus,
    pub(crate) brightness: Option<u64>,
    pub(crate) actual_brightness: Option<u64>,
    pub(crate) max_brightness: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RuntimeObservabilitySnapshot {
    pub(crate) mode: RuntimeObservabilityMode,
    pub(crate) status: ObservationStatus,
    pub(crate) collected_at: u64,
    pub(crate) reads_attempted: u64,
    pub(crate) wakeup_sources: Vec<WakeupSourceObservation>,
    pub(crate) runtime_pm: Vec<RuntimePmObservation>,
    pub(crate) cpu_idle: Vec<CpuIdleObservation>,
    pub(crate) pm_qos: PmQosObservation,
    pub(crate) storage: Vec<StorageObservation>,
    pub(crate) backlights: Vec<BacklightObservation>,
}

impl RuntimeObservabilitySnapshot {
    pub(crate) fn collect(
        read: &dyn KernelRead,
        clock: &dyn Clock,
        mode: RuntimeObservabilityMode,
        previous: Option<&Self>,
    ) -> Self {
        let collected_at = clock.now_unix();
        if mode == RuntimeObservabilityMode::Off {
            return Self {
                mode,
                status: ObservationStatus::Disabled,
                collected_at,
                pm_qos: PmQosObservation {
                    status: ObservationStatus::Disabled,
                    ..PmQosObservation::default()
                },
                ..Self::default()
            };
        }

        let stale = previous.is_some_and(|previous| collected_at <= previous.collected_at);
        let mut reads = ReadCounter::default();
        let mut snapshot = Self {
            mode,
            status: if stale {
                ObservationStatus::Stale
            } else {
                ObservationStatus::Observed
            },
            collected_at,
            wakeup_sources: collect_wakeup_sources(read, &mut reads, previous, stale),
            runtime_pm: collect_runtime_pm(read, &mut reads, previous, stale),
            cpu_idle: collect_cpu_idle(read, &mut reads, previous, stale),
            pm_qos: collect_pm_qos(read, &mut reads),
            storage: collect_storage(read, &mut reads, previous),
            backlights: collect_backlights(read, &mut reads, previous),
            ..Self::default()
        };
        snapshot.reads_attempted = reads.attempts;
        snapshot
    }

    pub(crate) fn render_summary(&self) -> String {
        if self.mode == RuntimeObservabilityMode::Off {
            return "observability.runtime=off status=disabled reads=0\n".to_string();
        }

        let mut out = format!(
            "observability.runtime=observe status={} reads={} wakeups={} runtime_pm={} cpu_idle={} storage={} backlights={}\n",
            self.status.as_str(),
            self.reads_attempted,
            self.wakeup_sources.len(),
            self.runtime_pm.len(),
            self.cpu_idle.len(),
            self.storage.len(),
            self.backlights.len(),
        );
        out.push_str(&format!(
            "pm_qos.cpu_latency_us={} requestors={} status={}\n",
            self.pm_qos
                .effective_cpu_latency_us
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string()),
            self.pm_qos.requestor_count,
            self.pm_qos.status.as_str(),
        ));
        for source in &self.wakeup_sources {
            out.push_str(&format!(
                "wakeup.{} name={} events_delta={} wakeups_delta={} total_time_delta_us={} status={}\n",
                source.id,
                source.name,
                source.event_delta.render(),
                source.wakeup_delta.render(),
                source.total_time_delta_us.render(),
                source.status.as_str(),
            ));
        }
        for device in &self.runtime_pm {
            out.push_str(&format!(
                "runtime_pm.{}={} control={} active_delta_us={} suspended_delta_us={} resume_latency_us={} status={}\n",
                device.id,
                device.runtime_status.as_deref().unwrap_or("unavailable"),
                device.control.as_deref().unwrap_or("unavailable"),
                device.active_time_delta_us.render(),
                device.suspended_time_delta_us.render(),
                device
                    .resume_latency_constraint_us
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unavailable".to_string()),
                device.status.as_str(),
            ));
        }
        for idle in &self.cpu_idle {
            out.push_str(&format!(
                "cpuidle.cpu{}.{} time_delta_us={} usage_delta={} status={}\n",
                idle.cpu,
                idle.state,
                idle.time_delta_us.render(),
                idle.usage_delta.render(),
                idle.status.as_str(),
            ));
        }
        for storage in &self.storage {
            out.push_str(&format!(
                "storage.{}.{}={} status={}\n",
                storage.kind,
                storage.id,
                storage.state.as_deref().unwrap_or("unavailable"),
                storage.status.as_str(),
            ));
        }
        for backlight in &self.backlights {
            out.push_str(&format!(
                "backlight.{}={}/{} actual={} status={}\n",
                backlight.id,
                render_u64(backlight.brightness),
                render_u64(backlight.max_brightness),
                render_u64(backlight.actual_brightness),
                backlight.status.as_str(),
            ));
        }
        out
    }
}

fn render_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

#[derive(Default)]
struct ReadCounter {
    attempts: u64,
}

impl ReadCounter {
    fn exists(&mut self, read: &dyn KernelRead, path: &Path) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        read.exists(path)
    }

    fn read_to_string(&mut self, read: &dyn KernelRead, path: &Path) -> io::Result<String> {
        self.attempts = self.attempts.saturating_add(1);
        read.read_to_string(path)
    }

    fn read_dir(&mut self, read: &dyn KernelRead, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.attempts = self.attempts.saturating_add(1);
        read.read_dir(path)
    }
}

fn error_status(error: &io::Error) -> ObservationStatus {
    match error.kind() {
        io::ErrorKind::NotFound => ObservationStatus::Unsupported,
        io::ErrorKind::PermissionDenied => ObservationStatus::PermissionDenied,
        _ => ObservationStatus::Malformed,
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(sanitize)
        .unwrap_or_else(|| "unknown".to_string())
}

fn read_text(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    path: &Path,
) -> Result<String, ObservationStatus> {
    counter
        .read_to_string(read, path)
        .map(|value| value.trim().to_string())
        .map_err(|error| error_status(&error))
}

fn read_u64(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    path: &Path,
) -> Result<u64, ObservationStatus> {
    read_text(read, counter, path)?
        .parse()
        .map_err(|_| ObservationStatus::Malformed)
}

/// Several kernel duration counters are exported in milliseconds while the
/// rest of this snapshot is in microseconds. Convert once, at the read, so a
/// single canonical unit reaches every consumer: `cpuidle/state*/time` and
/// `power/pm_qos_resume_latency_us` are genuinely microseconds, and D1
/// reconciles runtime-PM residency against both.
///
/// Milliseconds is also all the resolution the kernel has here, so a converted
/// value is always a whole multiple of 1000. That is a real property of the
/// source, not lost precision introduced by the conversion.
fn read_ms_as_us(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    path: &Path,
) -> Result<u64, ObservationStatus> {
    read_u64(read, counter, path)
        .map(|milliseconds| milliseconds.saturating_mul(MICROSECONDS_PER_MILLISECOND))
}

/// Saturating rather than wrapping: a counter close to `u64::MAX` milliseconds
/// is not reachable on real hardware, and pinning the ceiling is safer than
/// reporting a wrapped duration as a small one.
const MICROSECONDS_PER_MILLISECOND: u64 = 1_000;

fn read_i64(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    path: &Path,
) -> Result<i64, ObservationStatus> {
    read_text(read, counter, path)?
        .parse()
        .map_err(|_| ObservationStatus::Malformed)
}

fn previous_wakeup<'a>(
    previous: Option<&'a RuntimeObservabilitySnapshot>,
    id: &str,
) -> Option<&'a WakeupSourceObservation> {
    previous?.wakeup_sources.iter().find(|entry| entry.id == id)
}

fn collect_wakeup_sources(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
    stale: bool,
) -> Vec<WakeupSourceObservation> {
    let root = Path::new("/sys/class/wakeup");
    if !counter.exists(read, root) {
        return stale_wakeups(previous);
    }
    let Ok(mut entries) = counter.read_dir(read, root) else {
        return stale_wakeups(previous);
    };
    entries.sort();
    let mut output = Vec::new();
    let mut present = BTreeSet::new();
    for path in entries {
        let id = basename(&path);
        present.insert(id.clone());
        let name = read_text(read, counter, &path.join("name")).unwrap_or_else(|_| id.clone());
        let mut status = ObservationStatus::Observed;
        let event_count = read_u64(read, counter, &path.join("event_count"))
            .map_err(|failure| status = status.merge(failure))
            .ok();
        let wakeup_count = read_u64(read, counter, &path.join("wakeup_count"))
            .map_err(|failure| status = status.merge(failure))
            .ok();
        // `/sys/class/wakeup/wakeupN/total_time_ms`, in milliseconds. The
        // debugfs `wakeup_sources` table has a column called `total_time` and
        // that name was mistaken for the sysfs file name; no `total_time` file
        // has ever existed here, so there is nothing to fall back to.
        let total_time_us = read_ms_as_us(read, counter, &path.join("total_time_ms"))
            .map_err(|failure| status = status.merge(failure))
            .ok();
        let prior = previous_wakeup(previous, &id);
        output.push(WakeupSourceObservation {
            id,
            name,
            status: if stale {
                ObservationStatus::Stale
            } else {
                status
            },
            event_count,
            event_delta: CounterDelta::between(
                event_count,
                prior.and_then(|entry| entry.event_count),
                stale,
            ),
            wakeup_count,
            wakeup_delta: CounterDelta::between(
                wakeup_count,
                prior.and_then(|entry| entry.wakeup_count),
                stale,
            ),
            total_time_us,
            total_time_delta_us: CounterDelta::between(
                total_time_us,
                prior.and_then(|entry| entry.total_time_us),
                stale,
            ),
        });
    }
    if let Some(previous) = previous {
        for prior in &previous.wakeup_sources {
            if !present.contains(&prior.id) {
                let mut vanished = prior.clone();
                vanished.status = ObservationStatus::Stale;
                vanished.event_delta = CounterDelta::default();
                vanished.wakeup_delta = CounterDelta::default();
                vanished.total_time_delta_us = CounterDelta::default();
                output.push(vanished);
            }
        }
    }
    output.sort_by(|left, right| left.id.cmp(&right.id));
    output
}

fn stale_wakeups(previous: Option<&RuntimeObservabilitySnapshot>) -> Vec<WakeupSourceObservation> {
    previous
        .map(|previous| {
            previous
                .wakeup_sources
                .iter()
                .cloned()
                .map(|mut entry| {
                    entry.status = ObservationStatus::Stale;
                    entry.event_delta = CounterDelta::default();
                    entry.wakeup_delta = CounterDelta::default();
                    entry.total_time_delta_us = CounterDelta::default();
                    entry
                })
                .collect()
        })
        .unwrap_or_default()
}

fn previous_runtime_pm<'a>(
    previous: Option<&'a RuntimeObservabilitySnapshot>,
    id: &str,
) -> Option<&'a RuntimePmObservation> {
    previous?.runtime_pm.iter().find(|entry| entry.id == id)
}

fn collect_runtime_pm(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
    stale: bool,
) -> Vec<RuntimePmObservation> {
    let mut output = Vec::new();
    let mut present = BTreeSet::new();
    for bus in ["pci", "usb", "platform", "i2c", "hid"] {
        let root = PathBuf::from(format!("/sys/bus/{bus}/devices"));
        if !counter.exists(read, &root) {
            continue;
        }
        let Ok(mut entries) = counter.read_dir(read, &root) else {
            continue;
        };
        entries.sort();
        for path in entries {
            let runtime_status_path = path.join("power/runtime_status");
            if !counter.exists(read, &runtime_status_path) {
                continue;
            }
            let id = format!("{bus}:{}", basename(&path));
            present.insert(id.clone());
            let mut status = ObservationStatus::Observed;
            let runtime_status = read_text(read, counter, &runtime_status_path)
                .map_err(|failure| status = status.merge(failure))
                .ok()
                .and_then(|value| match value.as_str() {
                    "active" | "suspended" | "suspending" | "resuming" | "error" => Some(value),
                    // The sixth documented value. Report it as given: an
                    // operator needs to tell a device with no runtime PM apart
                    // from one whose sysfs could not be read, and dropping the
                    // string would erase exactly that distinction.
                    "unsupported" => {
                        status = status.merge(ObservationStatus::NotApplicable);
                        Some(value)
                    }
                    _ => {
                        status = status.merge(ObservationStatus::Malformed);
                        None
                    }
                });
            let control = read_text(read, counter, &path.join("power/control"))
                .map_err(|failure| status = status.merge(failure))
                .ok();
            // Both residency counters are milliseconds
            // (`drivers/base/power/sysfs.c` divides by `NSEC_PER_MSEC`), even
            // though the neighbouring `pm_qos_resume_latency_us` is
            // microseconds. Measured on hardware: a 5.001 s wall-clock wait
            // advanced `runtime_active_time` by 5001.
            let active_time_us =
                read_ms_as_us(read, counter, &path.join("power/runtime_active_time"))
                    .map_err(|failure| status = status.merge(failure))
                    .ok();
            let suspended_time_us =
                read_ms_as_us(read, counter, &path.join("power/runtime_suspended_time"))
                    .map_err(|failure| status = status.merge(failure))
                    .ok();
            let qos_path = path.join("power/pm_qos_resume_latency_us");
            let resume_latency_constraint_us = match read_i64(read, counter, &qos_path) {
                Ok(value) => Some(value),
                // This optional attribute is absent on many devices. Only
                // absence is benign; an unreadable or corrupt constraint is
                // not a successfully observed device.
                Err(ObservationStatus::Unsupported) => None,
                Err(failure) => {
                    status = status.merge(failure);
                    None
                }
            };
            let prior = previous_runtime_pm(previous, &id);
            output.push(RuntimePmObservation {
                id,
                status: if stale {
                    ObservationStatus::Stale
                } else {
                    status
                },
                runtime_status,
                control,
                active_time_us,
                active_time_delta_us: CounterDelta::between(
                    active_time_us,
                    prior.and_then(|entry| entry.active_time_us),
                    stale,
                ),
                suspended_time_us,
                suspended_time_delta_us: CounterDelta::between(
                    suspended_time_us,
                    prior.and_then(|entry| entry.suspended_time_us),
                    stale,
                ),
                resume_latency_constraint_us,
            });
        }
    }
    if let Some(previous) = previous {
        for prior in &previous.runtime_pm {
            if !present.contains(&prior.id) {
                let mut vanished = prior.clone();
                vanished.status = ObservationStatus::Stale;
                vanished.active_time_delta_us = CounterDelta::default();
                vanished.suspended_time_delta_us = CounterDelta::default();
                output.push(vanished);
            }
        }
    }
    output.sort_by(|left, right| left.id.cmp(&right.id));
    output
}

fn previous_cpu_idle<'a>(
    previous: Option<&'a RuntimeObservabilitySnapshot>,
    cpu: u32,
    state: &str,
) -> Option<&'a CpuIdleObservation> {
    previous?
        .cpu_idle
        .iter()
        .find(|entry| entry.cpu == cpu && entry.state == state)
}

fn collect_cpu_idle(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
    stale: bool,
) -> Vec<CpuIdleObservation> {
    let root = Path::new("/sys/devices/system/cpu");
    if !counter.exists(read, root) {
        return Vec::new();
    }
    let Ok(mut cpus) = counter.read_dir(read, root) else {
        return Vec::new();
    };
    cpus.sort();
    let mut output = Vec::new();
    let mut present = BTreeSet::new();
    for cpu_path in cpus {
        let cpu_name = basename(&cpu_path);
        let Some(cpu) = cpu_name
            .strip_prefix("cpu")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let idle_root = cpu_path.join("cpuidle");
        if !counter.exists(read, &idle_root) {
            continue;
        }
        let Ok(mut states) = counter.read_dir(read, &idle_root) else {
            continue;
        };
        states.sort();
        for state_path in states {
            let mut status = ObservationStatus::Observed;
            let state = read_text(read, counter, &state_path.join("name"))
                .map(|value| sanitize(&value))
                .map_err(|failure| status = status.merge(failure))
                .unwrap_or_else(|_| basename(&state_path));
            present.insert((cpu, state.clone()));
            let time_us = read_u64(read, counter, &state_path.join("time"))
                .map_err(|failure| status = status.merge(failure))
                .ok();
            let usage = read_u64(read, counter, &state_path.join("usage"))
                .map_err(|failure| status = status.merge(failure))
                .ok();
            let prior = previous_cpu_idle(previous, cpu, &state);
            output.push(CpuIdleObservation {
                cpu,
                state,
                status: if stale {
                    ObservationStatus::Stale
                } else {
                    status
                },
                time_us,
                time_delta_us: CounterDelta::between(
                    time_us,
                    prior.and_then(|entry| entry.time_us),
                    stale,
                ),
                usage,
                usage_delta: CounterDelta::between(
                    usage,
                    prior.and_then(|entry| entry.usage),
                    stale,
                ),
            });
        }
    }
    if let Some(previous) = previous {
        for prior in &previous.cpu_idle {
            if !present.contains(&(prior.cpu, prior.state.clone())) {
                let mut vanished = prior.clone();
                vanished.status = ObservationStatus::Stale;
                vanished.time_delta_us = CounterDelta::default();
                vanished.usage_delta = CounterDelta::default();
                output.push(vanished);
            }
        }
    }
    output.sort_by(|left, right| (left.cpu, &left.state).cmp(&(right.cpu, &right.state)));
    output
}

fn collect_pm_qos(read: &dyn KernelRead, counter: &mut ReadCounter) -> PmQosObservation {
    let path = Path::new("/sys/kernel/debug/pm_qos/cpu_latency_constraints");
    // Open directly: exists() folds permission errors on debugfs ancestors
    // into false and would misreport an inaccessible interface as absent.
    let text = match counter.read_to_string(read, path) {
        Ok(text) => text,
        Err(error) => {
            return PmQosObservation {
                status: error_status(&error),
                ..PmQosObservation::default()
            };
        }
    };
    let mut values = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some(value) = line
            .split_whitespace()
            .rev()
            .find_map(|token| token.trim_end_matches(',').parse::<i64>().ok())
        else {
            return PmQosObservation {
                status: ObservationStatus::Malformed,
                ..PmQosObservation::default()
            };
        };
        values.push(value);
    }
    PmQosObservation {
        status: ObservationStatus::Observed,
        effective_cpu_latency_us: values.iter().copied().min(),
        requestor_count: values.len(),
    }
}

fn collect_storage(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
) -> Vec<StorageObservation> {
    let mut output = Vec::new();
    let mut present = BTreeSet::new();

    let sata_root = Path::new("/sys/class/scsi_host");
    if counter.exists(read, sata_root) {
        if let Ok(mut hosts) = counter.read_dir(read, sata_root) {
            hosts.sort();
            for host in hosts {
                let path = host.join("link_power_management_policy");
                if !counter.exists(read, &path) {
                    continue;
                }
                push_storage(
                    read,
                    counter,
                    &mut output,
                    &mut present,
                    "sata_alpm",
                    basename(&host),
                    path,
                );
            }
        }
    }

    let pci_root = Path::new("/sys/bus/pci/devices");
    if counter.exists(read, pci_root) {
        if let Ok(mut devices) = counter.read_dir(read, pci_root) {
            devices.sort();
            for device in devices {
                let path = device.join("link/l1_aspm");
                if !counter.exists(read, &path) {
                    continue;
                }
                push_storage(
                    read,
                    counter,
                    &mut output,
                    &mut present,
                    "pcie_aspm",
                    basename(&device),
                    path,
                );
            }
        }
    }

    let nvme_root = Path::new("/sys/class/nvme");
    if counter.exists(read, nvme_root) {
        if let Ok(mut controllers) = counter.read_dir(read, nvme_root) {
            controllers.sort();
            for controller in controllers {
                let direct = controller.join("power/runtime_status");
                let nested = controller.join("device/power/runtime_status");
                let path = if counter.exists(read, &direct) {
                    Some(direct)
                } else if counter.exists(read, &nested) {
                    Some(nested)
                } else {
                    None
                };
                if let Some(path) = path {
                    push_storage(
                        read,
                        counter,
                        &mut output,
                        &mut present,
                        "nvme_runtime",
                        basename(&controller),
                        path,
                    );
                }
            }
        }
    }

    if let Some(previous) = previous {
        for prior in &previous.storage {
            if !present.contains(&(prior.kind.clone(), prior.id.clone())) {
                let mut vanished = prior.clone();
                vanished.status = ObservationStatus::Stale;
                output.push(vanished);
            }
        }
    }
    output.sort_by(|left, right| (&left.kind, &left.id).cmp(&(&right.kind, &right.id)));
    output
}

fn push_storage(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    output: &mut Vec<StorageObservation>,
    present: &mut BTreeSet<(String, String)>,
    kind: &str,
    id: String,
    path: PathBuf,
) {
    present.insert((kind.to_string(), id.clone()));
    let (state, status) = match read_text(read, counter, &path) {
        Ok(value) => (Some(value), ObservationStatus::Observed),
        Err(status) => (None, status),
    };
    output.push(StorageObservation {
        id,
        kind: kind.to_string(),
        status,
        state,
    });
}

fn collect_backlights(
    read: &dyn KernelRead,
    counter: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
) -> Vec<BacklightObservation> {
    let root = Path::new("/sys/class/backlight");
    if !counter.exists(read, root) {
        return previous
            .map(|previous| {
                previous
                    .backlights
                    .iter()
                    .cloned()
                    .map(|mut entry| {
                        entry.status = ObservationStatus::Stale;
                        entry
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    let Ok(mut entries) = counter.read_dir(read, root) else {
        return Vec::new();
    };
    entries.sort();
    let mut output = Vec::new();
    let mut present = BTreeSet::new();
    for path in entries {
        let id = basename(&path);
        present.insert(id.clone());
        let mut status = ObservationStatus::Observed;
        let brightness = read_u64(read, counter, &path.join("brightness"))
            .map_err(|failure| status = status.merge(failure))
            .ok();
        let actual_path = path.join("actual_brightness");
        // Requested brightness is not evidence of the actual device state.
        let actual_brightness = read_u64(read, counter, &actual_path)
            .map_err(|failure| status = status.merge(failure))
            .ok();
        let max_brightness = read_u64(read, counter, &path.join("max_brightness"))
            .map_err(|failure| status = status.merge(failure))
            .ok();
        output.push(BacklightObservation {
            id,
            status,
            brightness,
            actual_brightness,
            max_brightness,
        });
    }
    if let Some(previous) = previous {
        for prior in &previous.backlights {
            if !present.contains(&prior.id) {
                let mut vanished = prior.clone();
                vanished.status = ObservationStatus::Stale;
                output.push(vanished);
            }
        }
    }
    output.sort_by(|left, right| left.id.cmp(&right.id));
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    /// Read-only fixture kernel.
    ///
    /// O1 never writes, so its tests need only the read seam and a clock. A
    /// local fixture keeps the reporter's test build free of the write and
    /// fault-injection machinery it does not own.
    #[derive(Default)]
    struct ObserverKernel {
        files: RefCell<BTreeMap<PathBuf, String>>,
        directories: RefCell<BTreeMap<PathBuf, Vec<PathBuf>>>,
        read_faults: RefCell<BTreeMap<PathBuf, io::ErrorKind>>,
        /// Paths that exist but that the invoking user may not read. A real
        /// capture taken without root records these, so the fixture can show
        /// the reporter the same wall it meets in production.
        unreadable: RefCell<BTreeSet<PathBuf>>,
        now: Cell<u64>,
    }

    impl ObserverKernel {
        fn new() -> Self {
            Self {
                now: Cell::new(1_000),
                ..Self::default()
            }
        }

        fn write_raw(&self, path: &Path, value: &str) {
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), value.to_string());
        }

        /// Drop a file so a test can show the reporter a kernel that does not
        /// export it.
        fn remove_raw(&self, path: &Path) {
            self.files.borrow_mut().remove(path);
        }

        fn add_unreadable(&self, path: &Path) {
            self.unreadable.borrow_mut().insert(path.to_path_buf());
        }

        fn add_dir_entry(&self, directory: &Path, entry: &Path) {
            self.directories
                .borrow_mut()
                .entry(directory.to_path_buf())
                .or_default()
                .push(entry.to_path_buf());
        }

        fn advance_clock(&self, seconds: u64) {
            self.now.set(self.now.get() + seconds);
        }

        /// Make the next read of `path` fail. Used to prove that a refused
        /// read is reported as `permission_denied` rather than folded into
        /// `unsupported` or a fabricated value.
        fn fail_next_read(&self, path: PathBuf, error: io::ErrorKind) {
            self.read_faults.borrow_mut().insert(path, error);
        }
    }

    impl KernelRead for ObserverKernel {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            if let Some(error) = self.read_faults.borrow_mut().remove(path) {
                return Err(io::Error::new(error, "injected read fault"));
            }
            if self.unreadable.borrow().contains(path) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "captured as unreadable by an unprivileged reader",
                ));
            }
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such fixture file"))
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
            if let Some(error) = self.read_faults.borrow_mut().remove(path) {
                return Err(io::Error::new(error, "injected read fault"));
            }
            self.directories
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such fixture directory"))
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
                || self.directories.borrow().contains_key(path)
                || self.unreadable.borrow().contains(path)
        }
    }

    impl Clock for ObserverKernel {
        fn now_unix(&self) -> u64 {
            self.now.get()
        }
    }

    fn add_file(kernel: &ObserverKernel, path: &str, value: &str) {
        kernel.write_raw(Path::new(path), value);
    }

    fn add_dir(kernel: &ObserverKernel, directory: &str, entry: &str) {
        kernel.add_dir_entry(Path::new(directory), Path::new(entry));
    }

    fn remove_file(kernel: &ObserverKernel, path: &str) {
        kernel.remove_raw(Path::new(path));
    }

    /// A real `/sys` layout, taken from a physical host by
    /// `tools/capture-o1-sysfs.sh` and committed verbatim. Both defects O1's
    /// cold verification found were fixture bugs before they were code bugs:
    /// the hand-written fixture asserted a sysfs layout that does not exist,
    /// so the tests proved the arithmetic and never the interface. A captured
    /// layout cannot make that mistake, because nobody typed it.
    const CAPTURED_SYSFS: &str =
        include_str!("../tests/fixtures/o1-sysfs-capture-victus-2026-09-04.txt");

    /// Rebuild the captured host in the read-only fixture kernel.
    ///
    /// Format, one record per line: `D<TAB>dir<TAB>entry`,
    /// `F<TAB>path<TAB>value` (with `\n` for an embedded newline), or
    /// `U<TAB>path` for a path that exists but that an unprivileged reader
    /// cannot open. Lines starting with `#` are provenance.
    fn captured_fixture() -> ObserverKernel {
        let kernel = ObserverKernel::new();
        for (number, line) in CAPTURED_SYSFS.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            match (fields.next(), fields.next(), fields.next()) {
                (Some("D"), Some(directory), Some(entry)) => {
                    kernel.add_dir_entry(Path::new(directory), Path::new(entry));
                }
                (Some("F"), Some(path), Some(value)) => {
                    // Restore the trailing newline every sysfs read carries;
                    // the capture strips it so one file is one line.
                    let value = format!("{}\n", value.replace("\\n", "\n"));
                    kernel.write_raw(Path::new(path), &value);
                }
                (Some("U"), Some(path), None) => {
                    kernel.add_unreadable(Path::new(path));
                }
                _ => panic!(
                    "captured fixture line {} is not a D/F/U record: {line:?}",
                    number + 1
                ),
            }
        }
        kernel
    }

    fn observe(kernel: &ObserverKernel) -> RuntimeObservabilitySnapshot {
        RuntimeObservabilitySnapshot::collect(
            kernel,
            kernel,
            RuntimeObservabilityMode::Observe,
            None,
        )
    }

    fn fixture() -> ObserverKernel {
        let kernel = ObserverKernel::new();
        add_dir(&kernel, "/sys/class/wakeup", "/sys/class/wakeup/wakeup0");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/name", "XHC\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/event_count", "10\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/wakeup_count", "3\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/total_time_ms", "1200\n");

        add_dir(
            &kernel,
            "/sys/bus/pci/devices",
            "/sys/bus/pci/devices/0000:00:1f.0",
        );
        add_file(
            &kernel,
            "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_status",
            "suspended\n",
        );
        add_file(
            &kernel,
            "/sys/bus/pci/devices/0000:00:1f.0/power/control",
            "auto\n",
        );
        add_file(
            &kernel,
            "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_active_time",
            "200\n",
        );
        add_file(
            &kernel,
            "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_suspended_time",
            "800\n",
        );
        add_file(
            &kernel,
            "/sys/bus/pci/devices/0000:00:1f.0/power/pm_qos_resume_latency_us",
            "5000\n",
        );
        add_file(
            &kernel,
            "/sys/bus/pci/devices/0000:00:1f.0/link/l1_aspm",
            "1\n",
        );

        add_dir(
            &kernel,
            "/sys/devices/system/cpu",
            "/sys/devices/system/cpu/cpu0",
        );
        add_dir(
            &kernel,
            "/sys/devices/system/cpu/cpu0/cpuidle",
            "/sys/devices/system/cpu/cpu0/cpuidle/state0",
        );
        add_file(
            &kernel,
            "/sys/devices/system/cpu/cpu0/cpuidle/state0/name",
            "C6\n",
        );
        add_file(
            &kernel,
            "/sys/devices/system/cpu/cpu0/cpuidle/state0/time",
            "1000\n",
        );
        add_file(
            &kernel,
            "/sys/devices/system/cpu/cpu0/cpuidle/state0/usage",
            "20\n",
        );

        add_file(
            &kernel,
            "/sys/kernel/debug/pm_qos/cpu_latency_constraints",
            "101 audio 100\n202 game 50\n",
        );

        add_dir(
            &kernel,
            "/sys/class/scsi_host",
            "/sys/class/scsi_host/host0",
        );
        add_file(
            &kernel,
            "/sys/class/scsi_host/host0/link_power_management_policy",
            "med_power_with_dipm\n",
        );

        add_dir(&kernel, "/sys/class/nvme", "/sys/class/nvme/nvme0");
        add_file(
            &kernel,
            "/sys/class/nvme/nvme0/device/power/runtime_status",
            "suspended\n",
        );

        add_dir(
            &kernel,
            "/sys/class/backlight",
            "/sys/class/backlight/intel_backlight",
        );
        add_file(
            &kernel,
            "/sys/class/backlight/intel_backlight/brightness",
            "400\n",
        );
        add_file(
            &kernel,
            "/sys/class/backlight/intel_backlight/actual_brightness",
            "390\n",
        );
        add_file(
            &kernel,
            "/sys/class/backlight/intel_backlight/max_brightness",
            "1000\n",
        );
        kernel
    }

    #[test]
    fn o1_runtime_mode_defaults_to_observe_and_parses_off() {
        assert_eq!(
            RuntimeConfig::default().mode,
            RuntimeObservabilityMode::Observe
        );
        let parsed: PolicyFragment =
            toml::from_str("[observability.runtime]\nmode = \"off\"\n[unrelated]\nvalue = 1\n")
                .unwrap();
        assert_eq!(
            parsed.observability.runtime.mode,
            RuntimeObservabilityMode::Off
        );
    }

    #[test]
    fn o1_pm_qos_inaccessible_ancestor_is_permission_denied() {
        let kernel = ObserverKernel::new();
        // Like Path::exists() behind an inaccessible debugfs ancestor, this
        // absent fixture path returns false from exists but EACCES on open.
        kernel.fail_next_read(
            PathBuf::from("/sys/kernel/debug/pm_qos/cpu_latency_constraints"),
            io::ErrorKind::PermissionDenied,
        );
        let snapshot = RuntimeObservabilitySnapshot::collect(
            &kernel,
            &kernel,
            RuntimeObservabilityMode::Observe,
            None,
        );
        assert_eq!(snapshot.pm_qos.status, ObservationStatus::PermissionDenied);
        assert!(snapshot
            .render_summary()
            .contains("pm_qos.cpu_latency_us=unavailable requestors=0 status=permission_denied"));
    }

    #[test]
    fn o1_runtime_pm_qos_read_failures_are_not_observed() {
        let path = Path::new("/sys/bus/pci/devices/0000:00:1f.0/power/pm_qos_resume_latency_us");
        for (failure, expected) in [
            (
                io::ErrorKind::PermissionDenied,
                ObservationStatus::PermissionDenied,
            ),
            (io::ErrorKind::InvalidData, ObservationStatus::Malformed),
        ] {
            let kernel = fixture();
            kernel.fail_next_read(path.to_path_buf(), failure);
            let snapshot = RuntimeObservabilitySnapshot::collect(
                &kernel,
                &kernel,
                RuntimeObservabilityMode::Observe,
                None,
            );
            assert_eq!(snapshot.runtime_pm[0].status, expected);
            assert_eq!(snapshot.runtime_pm[0].resume_latency_constraint_us, None);
        }
    }

    #[test]
    fn o1_missing_actual_brightness_is_not_requested_brightness() {
        let kernel = fixture();
        remove_file(
            &kernel,
            "/sys/class/backlight/intel_backlight/actual_brightness",
        );
        let snapshot = RuntimeObservabilitySnapshot::collect(
            &kernel,
            &kernel,
            RuntimeObservabilityMode::Observe,
            None,
        );
        assert!(snapshot.backlights[0].brightness.is_some());
        assert_eq!(snapshot.backlights[0].actual_brightness, None);
        assert_eq!(
            snapshot.backlights[0].status,
            ObservationStatus::Unsupported
        );
        assert!(snapshot.render_summary().contains("actual=unavailable"));
    }

    #[test]
    fn o1_mock_full_state_snapshot_is_deterministic() {
        let kernel = fixture();
        let left = RuntimeObservabilitySnapshot::collect(
            &kernel,
            &kernel,
            RuntimeObservabilityMode::Observe,
            None,
        );
        let right = RuntimeObservabilitySnapshot::collect(
            &kernel,
            &kernel,
            RuntimeObservabilityMode::Observe,
            None,
        );
        assert_eq!(left, right);
        assert_eq!(left.wakeup_sources.len(), 1);
        assert_eq!(left.runtime_pm.len(), 1);
        assert_eq!(left.cpu_idle.len(), 1);
        assert_eq!(left.pm_qos.effective_cpu_latency_us, Some(50));
        assert_eq!(left.storage.len(), 3);
        assert_eq!(left.backlights.len(), 1);
        assert!(left.reads_attempted > 0);
    }

    #[test]
    fn o1_monotonic_counters_produce_real_deltas() {
        let first = fixture();
        let previous = RuntimeObservabilitySnapshot::collect(
            &first,
            &first,
            RuntimeObservabilityMode::Observe,
            None,
        );
        let second = fixture();
        second.advance_clock(2);
        add_file(&second, "/sys/class/wakeup/wakeup0/event_count", "14\n");
        add_file(
            &second,
            "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_suspended_time",
            "900\n",
        );
        let current = RuntimeObservabilitySnapshot::collect(
            &second,
            &second,
            RuntimeObservabilityMode::Observe,
            Some(&previous),
        );
        assert_eq!(current.wakeup_sources[0].event_delta.value, Some(4));
        // 800 ms -> 900 ms of suspended residency is 100_000 microseconds.
        // The kernel counter is milliseconds; the field is microseconds.
        assert_eq!(
            current.runtime_pm[0].suspended_time_delta_us.value,
            Some(100_000)
        );
    }

    #[test]
    fn o1_counter_reset_or_wrap_never_becomes_a_huge_delta() {
        let first = fixture();
        let previous = RuntimeObservabilitySnapshot::collect(
            &first,
            &first,
            RuntimeObservabilityMode::Observe,
            None,
        );
        let second = fixture();
        second.advance_clock(2);
        add_file(&second, "/sys/class/wakeup/wakeup0/event_count", "1\n");
        add_file(
            &second,
            "/sys/devices/system/cpu/cpu0/cpuidle/state0/time",
            "5\n",
        );
        let current = RuntimeObservabilitySnapshot::collect(
            &second,
            &second,
            RuntimeObservabilityMode::Observe,
            Some(&previous),
        );
        assert!(current.wakeup_sources[0].event_delta.reset_or_wrap);
        assert_eq!(current.wakeup_sources[0].event_delta.value, None);
        assert!(current.cpu_idle[0].time_delta_us.reset_or_wrap);
    }

    #[test]
    fn o1_disappearing_devices_are_reported_stale() {
        let first = fixture();
        let previous = RuntimeObservabilitySnapshot::collect(
            &first,
            &first,
            RuntimeObservabilityMode::Observe,
            None,
        );
        let second = ObserverKernel::new();
        second.advance_clock(2);
        let current = RuntimeObservabilitySnapshot::collect(
            &second,
            &second,
            RuntimeObservabilityMode::Observe,
            Some(&previous),
        );
        assert!(current
            .wakeup_sources
            .iter()
            .all(|entry| entry.status == ObservationStatus::Stale));
        assert!(current
            .runtime_pm
            .iter()
            .all(|entry| entry.status == ObservationStatus::Stale));
        assert!(current
            .backlights
            .iter()
            .all(|entry| entry.status == ObservationStatus::Stale));
    }

    #[test]
    fn o1_permission_error_is_distinct_from_unsupported_and_malformed() {
        let fault = fixture();
        fault.fail_next_read(
            PathBuf::from("/sys/class/wakeup/wakeup0/event_count"),
            io::ErrorKind::PermissionDenied,
        );
        let permission = RuntimeObservabilitySnapshot::collect(
            &fault,
            &fault,
            RuntimeObservabilityMode::Observe,
            None,
        );
        assert_eq!(
            permission.wakeup_sources[0].status,
            ObservationStatus::PermissionDenied
        );

        let empty = ObserverKernel::new();
        let unsupported = RuntimeObservabilitySnapshot::collect(
            &empty,
            &empty,
            RuntimeObservabilityMode::Observe,
            None,
        );
        assert_eq!(unsupported.pm_qos.status, ObservationStatus::Unsupported);

        let malformed = fixture();
        add_file(
            &malformed,
            "/sys/class/backlight/intel_backlight/brightness",
            "not-a-number\n",
        );
        let malformed = RuntimeObservabilitySnapshot::collect(
            &malformed,
            &malformed,
            RuntimeObservabilityMode::Observe,
            None,
        );
        assert_eq!(malformed.backlights[0].status, ObservationStatus::Malformed);
    }

    #[test]
    fn o1_nonadvancing_clock_marks_snapshot_stale_and_suppresses_deltas() {
        let first = fixture();
        let previous = RuntimeObservabilitySnapshot::collect(
            &first,
            &first,
            RuntimeObservabilityMode::Observe,
            None,
        );
        let second = fixture();
        add_file(&second, "/sys/class/wakeup/wakeup0/event_count", "20\n");
        let current = RuntimeObservabilitySnapshot::collect(
            &second,
            &second,
            RuntimeObservabilityMode::Observe,
            Some(&previous),
        );
        assert_eq!(current.status, ObservationStatus::Stale);
        assert_eq!(current.wakeup_sources[0].event_delta.value, None);
    }

    #[test]
    fn o1_wakeup_total_time_comes_from_the_kernel_ms_file_in_microseconds() {
        // `/sys/class/wakeup/wakeupN/total_time_ms` is milliseconds; the field
        // is microseconds. 1200 ms is 1_200_000 us, and the entry stays fully
        // observed because nothing failed to read.
        let kernel = fixture();
        let snapshot = observe(&kernel);
        assert_eq!(snapshot.wakeup_sources[0].total_time_us, Some(1_200_000));
        assert_eq!(
            snapshot.wakeup_sources[0].status,
            ObservationStatus::Observed
        );

        // A kernel that exports only the name the pre-repair code used must
        // degrade, not silently succeed. No `total_time` file has ever existed
        // under /sys/class/wakeup, so there is deliberately no fallback: a
        // fallback would let the wrong name keep working and hide the unit.
        let legacy = fixture();
        remove_file(&legacy, "/sys/class/wakeup/wakeup0/total_time_ms");
        add_file(&legacy, "/sys/class/wakeup/wakeup0/total_time", "1200\n");
        let degraded = observe(&legacy);
        assert_eq!(degraded.wakeup_sources[0].total_time_us, None);
        assert_eq!(
            degraded.wakeup_sources[0].status,
            ObservationStatus::Unsupported
        );
    }

    #[test]
    fn o1_runtime_pm_residency_is_converted_from_kernel_milliseconds() {
        let first = fixture();
        let previous = observe(&first);
        // 200 ms active and 800 ms suspended, reported in microseconds.
        assert_eq!(previous.runtime_pm[0].active_time_us, Some(200_000));
        assert_eq!(previous.runtime_pm[0].suspended_time_us, Some(800_000));
        // The neighbouring per-device PM QoS file really is microseconds and
        // is not converted, which is why the conversion has to be per-file.
        assert_eq!(
            previous.runtime_pm[0].resume_latency_constraint_us,
            Some(5000)
        );

        let second = fixture();
        second.advance_clock(2);
        add_file(
            &second,
            "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_suspended_time",
            "900\n",
        );
        let current = RuntimeObservabilitySnapshot::collect(
            &second,
            &second,
            RuntimeObservabilityMode::Observe,
            Some(&previous),
        );
        assert_eq!(
            current.runtime_pm[0].suspended_time_delta_us.value,
            Some(100_000)
        );
    }

    #[test]
    fn o1_unsupported_runtime_status_is_not_applicable_not_malformed() {
        let kernel = captured_fixture();
        let snapshot = observe(&kernel);
        let device = snapshot
            .runtime_pm
            .iter()
            .find(|entry| entry.id == "i2c:11-0050")
            .expect("captured host has an i2c device with no runtime PM");

        // The kernel answered the question. Report its answer, do not call it
        // corrupt and do not drop it.
        assert_eq!(device.runtime_status.as_deref(), Some("unsupported"));
        assert_eq!(device.status, ObservationStatus::NotApplicable);
        // And do not mask the fields that read cleanly.
        assert_eq!(device.control.as_deref(), Some("auto"));
        assert_eq!(device.active_time_us, Some(0));
        assert_eq!(device.suspended_time_us, Some(0));

        let rendered = snapshot.render_summary();
        let line = rendered
            .lines()
            .find(|line| line.starts_with("runtime_pm.i2c:11-0050="))
            .expect("the device appears in the summary");
        assert!(
            line.contains("runtime_pm.i2c:11-0050=unsupported"),
            "{line}"
        );
        assert!(line.contains("control=auto"), "{line}");
        assert!(line.contains("status=not_applicable"), "{line}");

        // A value the kernel does not document is still malformed, and a
        // malformed value still outranks not-applicable in the merge.
        let corrupt = captured_fixture();
        add_file(
            &corrupt,
            "/sys/bus/i2c/devices/11-0050/power/runtime_status",
            "wedged\n",
        );
        let corrupt = observe(&corrupt);
        let device = corrupt
            .runtime_pm
            .iter()
            .find(|entry| entry.id == "i2c:11-0050")
            .expect("the device is still enumerated");
        assert_eq!(device.status, ObservationStatus::Malformed);
        assert_eq!(device.runtime_status, None);
    }

    #[test]
    fn o1_captured_real_sysfs_layout_reports_no_read_failure() {
        let kernel = captured_fixture();
        let snapshot = observe(&kernel);

        // Every wakeup source on the captured host reads cleanly. Before the
        // repair all of them reported `unsupported` with every delta
        // suppressed, because the code read a file name that does not exist.
        assert_eq!(snapshot.wakeup_sources.len(), 3);
        let degraded: Vec<&str> = snapshot
            .wakeup_sources
            .iter()
            .filter(|entry| entry.status != ObservationStatus::Observed)
            .map(|entry| entry.id.as_str())
            .collect();
        assert!(degraded.is_empty(), "degraded wakeup sources: {degraded:?}");
        assert!(snapshot
            .wakeup_sources
            .iter()
            .all(|entry| entry.total_time_us.is_some()));

        // No device on a healthy host is malformed. The three that report
        // `unsupported` are not applicable; the other five are observed.
        assert_eq!(snapshot.runtime_pm.len(), 8);
        let malformed: Vec<&str> = snapshot
            .runtime_pm
            .iter()
            .filter(|entry| entry.status == ObservationStatus::Malformed)
            .map(|entry| entry.id.as_str())
            .collect();
        assert!(malformed.is_empty(), "malformed devices: {malformed:?}");
        assert_eq!(
            snapshot
                .runtime_pm
                .iter()
                .filter(|entry| entry.status == ObservationStatus::NotApplicable)
                .count(),
            3
        );
        assert_eq!(
            snapshot
                .runtime_pm
                .iter()
                .filter(|entry| entry.status == ObservationStatus::Observed)
                .count(),
            5
        );

        assert_eq!(snapshot.cpu_idle.len(), 4);
        assert_eq!(snapshot.storage.len(), 11);
        assert_eq!(snapshot.backlights.len(), 1);
        assert_eq!(snapshot.status, ObservationStatus::Observed);

        // `/sys/kernel/debug` is root-only, so an unprivileged reporter
        // genuinely cannot see the PM QoS constraint list. That is a correct
        // `unsupported`, and the capture preserves it rather than inventing a
        // readable file.
        assert_eq!(snapshot.pm_qos.status, ObservationStatus::Unsupported);
        assert_eq!(snapshot.pm_qos.effective_cpu_latency_us, None);
    }

    #[test]
    fn o1_synthetic_fixture_uses_only_file_names_the_real_kernel_exports() {
        // Names the reporter reads that an unprivileged capture on this host
        // cannot contain: no device here publishes a per-device PM QoS
        // constraint, and /sys/kernel/debug is root-only. Both are real kernel
        // ABI and both carry their unit in the file name.
        //
        // This allowlist is the one hole in the fence, so it is fenced too: it
        // is pinned to exactly these two entries, and each is asserted absent
        // from the capture. A capture that does grow one of them invalidates
        // its own exemption and this test fails until the entry is dropped —
        // so the way to get a new name past the fence is to capture it, which
        // is the whole point.
        const UNREACHABLE_BY_CAPTURE: [&str; 2] =
            ["pm_qos_resume_latency_us", "cpu_latency_constraints"];

        // Compared as (parent directory, file name) pairs rather than bare
        // file names. A bare-name check would accept a real name under the
        // wrong directory — `/sys/class/wakeup/wakeup0/runtime_status`, say —
        // which is the same class of mistake as the defect this fences.
        fn shapes(kernel: &ObserverKernel) -> BTreeSet<(String, String)> {
            kernel
                .files
                .borrow()
                .keys()
                .map(|path| {
                    let parent = path
                        .parent()
                        .map(basename)
                        .unwrap_or_else(|| "unknown".to_string());
                    (parent, basename(path))
                })
                .collect()
        }

        let captured = shapes(&captured_fixture());
        assert_eq!(UNREACHABLE_BY_CAPTURE.len(), 2);
        for name in UNREACHABLE_BY_CAPTURE {
            assert!(
                !captured.iter().any(|(_, file)| file == name),
                "{name} is in the capture now, so its exemption is obsolete: \
                 remove it from UNREACHABLE_BY_CAPTURE"
            );
        }

        let mut invented: Vec<String> = shapes(&fixture())
            .into_iter()
            .filter(|shape| {
                !captured.contains(shape) && !UNREACHABLE_BY_CAPTURE.contains(&shape.1.as_str())
            })
            .map(|(parent, file)| format!("{parent}/{file}"))
            .collect();
        invented.sort();

        assert!(
            invented.is_empty(),
            "the synthetic fixture reads paths the captured kernel layout does \
             not export, which is how O1's wakeup defect survived a green \
             suite: {invented:?}"
        );
    }

    #[test]
    fn o1_off_mode_performs_no_runtime_reads() {
        let kernel = fixture();
        let snapshot = RuntimeObservabilitySnapshot::collect(
            &kernel,
            &kernel,
            RuntimeObservabilityMode::Off,
            None,
        );
        assert_eq!(snapshot.status, ObservationStatus::Disabled);
        assert_eq!(snapshot.reads_attempted, 0);
        assert!(snapshot.wakeup_sources.is_empty());
        assert!(snapshot.runtime_pm.is_empty());
    }
}
